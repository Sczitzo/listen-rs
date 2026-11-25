use crate::cleanup::{self, ResolvedTarget};
use crate::storage::{self, Favorite, PromptTarget};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

const MAX_TEXT_BYTES: usize = 1_048_576;
const FORMAT_VERSION: u8 = 1;

fn matches_editor_text(saved: &str, editor: &str) -> bool {
    saved == editor || saved.contains('\r') && saved.replace("\r\n", "\n").replace('\r', "\n") == editor
}

#[derive(Clone, Deserialize, Serialize)]
struct Revision {
    text: String,
    kind: String,
    #[serde(default)]
    by: Option<String>,
    #[serde(default)]
    score: Option<Assessment>,
    #[serde(default)]
    stale: bool,
}

const REQUIRED_CRITERIA: [&str; 6] = [
    "clarity",
    "context",
    "task",
    "constraints",
    "success_criteria",
    "format",
];

fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Clone, Deserialize, Serialize)]
struct Assessment {
    score: f64,
    criteria: BTreeMap<String, f64>,
    #[serde(default, deserialize_with = "null_as_default")]
    gaps: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    improved: String,
    #[serde(default)]
    by: String,
    #[serde(default)]
    feedback_id: Option<u64>,
}

impl Assessment {
    fn parse(output: &str, by: String, id: u64) -> Option<Self> {
        let trimmed = output.trim();
        let unfenced = if let Some(rest) = trimmed.strip_prefix("```") {
            let after_lang = rest.find('\n').map_or(rest, |idx| &rest[idx + 1..]);
            after_lang.trim().strip_suffix("```").unwrap_or(after_lang).trim()
        } else {
            trimmed
        };
        let parsed = serde_json::from_str::<Self>(unfenced).ok().or_else(|| {
            let start = unfenced.find('{')?;
            let end = unfenced.rfind('}')?;
            (start < end)
                .then(|| serde_json::from_str::<Self>(&unfenced[start..=end]).ok())
                .flatten()
        });
        let mut result = parsed?;
        if !(1.0..=10.0).contains(&result.score)
            || !REQUIRED_CRITERIA.iter().all(|key| {
                result
                    .criteria
                    .get(*key)
                    .is_some_and(|score| (1.0..=5.0).contains(score))
            })
        {
            return None;
        }
        result.criteria.retain(|key, _| REQUIRED_CRITERIA.contains(&key.as_str()));
        // Fast/small judge models occasionally emit `score` on the 1..5 criteria scale
        // (e.g., criteria avg 3.8/5 -> score: 4) instead of 1..10. Normalize that clearly
        // so Refine does not stall at 4/10 when criteria are strong.
        let avg_criteria = result.criteria.values().copied().sum::<f64>()
            / (result.criteria.len().max(1) as f64);
        if result.score <= 5.0
            && avg_criteria >= 3.2
            && (result.score - avg_criteria).abs() <= 0.65
        {
            result.score = (result.score * 2.0).clamp(1.0, 10.0);
        }
        result.improved = strip_echoed_draft_tags(&result.improved, "");
        result.by = by;
        result.feedback_id = Some(id);
        Some(result)
    }
}

#[derive(Clone, Deserialize, Serialize)]
struct SuggestedVersion {
    text: String,
    by: String,
    score: Option<Assessment>,
    based_on: Option<usize>,
    basis_score: Option<Assessment>,
}

#[derive(Clone, Deserialize, Serialize)]
struct Candidate {
    id: u64,
    source_context: u64,
    source_edit: i64,
    source_text: String,
    kind: String,
    versions: Vec<SuggestedVersion>,
}

impl Candidate {
    fn stale(&self, state: &State) -> bool {
        self.source_context != state.context
            || self.source_edit != state.edit_id
            || self.source_text != state.active.text()
    }
}

#[derive(Clone, Deserialize, Serialize)]
struct Draft {
    id: u64,
    revisions: Vec<Revision>,
    selected: Option<usize>,
    composer: String,
    favorite: Option<String>,
    #[serde(default)]
    pending: Option<Candidate>,
}

impl Draft {
    fn new(id: u64) -> Self {
        Self {
            id,
            revisions: Vec::new(),
            selected: None,
            composer: String::new(),
            favorite: None,
            pending: None,
        }
    }

    fn text(&self) -> &str {
        self.selected
            .map_or("", |i| self.revisions[i].text.as_str())
    }

    fn add_revision(&mut self, text: String, kind: &str) {
        self.selected = Some(self.revisions.len());
        self.revisions.push(Revision {
            text,
            kind: kind.into(),
            by: None,
            score: None,
            stale: false,
        });
    }

    fn has_work(&self) -> bool {
        self.pending.is_some()
            || !self.composer.trim().is_empty()
            || self.revisions.iter().any(|r| !r.text.trim().is_empty())
    }

    fn title(&self) -> String {
        let first = self.revisions.iter().find(|r| !r.text.trim().is_empty());
        first
            .map_or(self.composer.as_str(), |r| &r.text)
            .trim()
            .chars()
            .take(80)
            .collect()
    }
}

#[derive(Clone, Deserialize, Serialize)]
struct Retained {
    id: u64,
    text: String,
    message: String,
}

#[derive(Clone, Deserialize, Serialize)]
struct State {
    format: u8,
    sequence: u64,
    context: u64,
    edit_id: i64,
    composer_edit_id: i64,
    active: Draft,
    archived: Vec<Draft>,
    favorites: Vec<Favorite>,
    retained: Vec<Retained>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            format: FORMAT_VERSION,
            sequence: 1,
            context: 1,
            edit_id: -1,
            composer_edit_id: -1,
            active: Draft::new(1),
            archived: Vec::new(),
            favorites: Vec::new(),
            retained: Vec::new(),
        }
    }
}

impl State {
    fn next_id(&mut self) -> u64 {
        self.sequence += 1;
        self.sequence
    }

    fn change_context(&mut self) {
        self.context = self.next_id();
        self.edit_id = -1;
        self.composer_edit_id = -1;
    }

    fn switch_draft(&mut self, draft: Draft) {
        let old = std::mem::replace(&mut self.active, draft);
        if old.has_work() {
            self.archived.push(old);
        }
        self.change_context();
    }

    fn retain(&mut self, text: &str, message: &str) {
        let id = self.next_id();
        self.retained.push(Retained {
            id,
            text: text.to_owned(),
            message: message.to_owned(),
        });
    }

    fn validate(&self) -> bool {
        if self.format != FORMAT_VERSION
            || self.sequence >= (1u64 << 52)
            || self.context > self.sequence
        {
            return false;
        }
        std::iter::once(&self.active)
            .chain(&self.archived)
            .all(|draft| {
                draft.id <= self.sequence
                    && draft.selected.is_none_or(|i| i < draft.revisions.len())
                    && draft.pending.as_ref().is_none_or(|candidate| {
                        candidate.id <= self.sequence && !candidate.versions.is_empty()
                    })
            })
            && self.retained.iter().all(|r| r.id <= self.sequence)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StageKind {
    Engineer,
    Refine,
    Rebuild,
    Submit,
    Assess,
}

pub struct LocalRequest {
    pub id: u64,
    pub stage: StageKind,
    pub system: String,
    pub user_prompt: String,
    pub text: String,
    pub json_output: bool,
}

fn wrap_draft(text: &str) -> String {
    format!("<draft>\n{text}\n</draft>")
}

fn strip_echoed_draft_tags(output: &str, source: &str) -> String {
    let trimmed = output.trim();
    if source.trim_start().starts_with("<draft>") {
        return trimmed.to_owned();
    }
    if let Some(inner) = trimmed
        .strip_prefix("<draft>")
        .and_then(|rest| rest.strip_suffix("</draft>"))
    {
        return inner.trim().to_owned();
    }
    trimmed.to_owned()
}

fn fmt_score(v: f64) -> String {
    if (v - v.round()).abs() < 0.05 {
        format!("{:.0}", v.round())
    } else {
        format!("{v:.1}")
    }
}

fn sanitize_feedback_line(line: &str) -> String {
    line.trim()
        .replace("</feedback>", "")
        .replace("<feedback>", "")
        .replace("</draft>", "")
        .trim()
        .to_owned()
}

fn format_refine_feedback(score: &Assessment) -> String {
    let weak: Vec<String> = REQUIRED_CRITERIA
        .iter()
        .filter_map(|&name| {
            let value = *score.criteria.get(name)?;
            (value <= 3.0).then(|| format!("{name} ({}/5)", fmt_score(value)))
        })
        .collect();
    let gaps: Vec<String> = score
        .gaps
        .iter()
        .map(|g| sanitize_feedback_line(g))
        .filter(|g| !g.is_empty())
        .collect();
    let below_five: Vec<String> = if weak.is_empty() && gaps.is_empty() {
        REQUIRED_CRITERIA
            .iter()
            .filter_map(|&name| {
                let value = *score.criteria.get(name)?;
                (value < 5.0).then(|| format!("{name} ({}/5)", fmt_score(value)))
            })
            .collect()
    } else {
        Vec::new()
    };
    if weak.is_empty() && gaps.is_empty() && below_five.is_empty() {
        return String::new();
    }
    let mut lines = vec![format!("Score: {}/10", fmt_score(score.score))];
    if !weak.is_empty() {
        lines.push(format!("Weak criteria: {}", weak.join(", ")));
    } else if !below_five.is_empty() {
        lines.push(format!("Room to strengthen: {}", below_five.join(", ")));
    }
    for gap in gaps {
        lines.push(format!("- {gap}"));
    }
    lines.join("\n")
}

#[derive(Clone, Copy)]
enum LocalKind {
    Suggest {
        kind: &'static str,
        clear_instruction: bool,
    },
    Pass { refine: bool },
    Feedback { suggest: bool },
    Review {
        candidate: u64,
        version: usize,
    },
}

impl LocalKind {
    fn label(self) -> &'static str {
        match self {
            Self::Suggest { .. } => "writing",
            Self::Pass { refine: true } => "refining",
            Self::Pass { refine: false } => "engineering",
            Self::Feedback { .. } => "scoring",
            Self::Review { .. } => "reviewing",
        }
    }
}

struct LocalOperation {
    id: u64,
    kind: LocalKind,
    target: ResolvedTarget,
    context: u64,
    edit: i64,
    instruction: i64,
    model: String,
    proposal: Option<String>,
}

impl LocalOperation {
    fn current(&self, state: &State) -> bool {
        self.context == state.context
            && self.edit == state.edit_id
            && self.instruction == state.composer_edit_id
    }
}

struct PreparedPass {
    operation: LocalOperation,
    revision: Revision,
}

struct DraftPassResult {
    id: u64,
    source: u64,
    context: u64,
    message: &'static str,
}

pub struct Workspace {
    path: PathBuf,
    state: State,
    version: u64,
    read_only: bool,
    save_error: &'static str,
    saved_sequence: u64,
    favorite_undo: Option<(String, Favorite)>,
    local: Option<LocalOperation>,
    local_error: Option<(u64, &'static str, String)>,
    prepared_pass: Option<PreparedPass>,
    last_pass: Option<DraftPassResult>,
}

impl Workspace {
    pub fn load(profile: &Path) -> Self {
        let path = profile.join("workspace.json");
        let loaded = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<State>(&bytes)
                .ok()
                .filter(State::validate),
            Err(error) if error.kind() == ErrorKind::NotFound => migrate_legacy(profile).ok(),
            Err(_) => None,
        };
        let read_only = loaded.is_none();
        let state = loaded.unwrap_or_default();
        Self {
            path,
            saved_sequence: state.sequence,
            state,
            version: 0,
            read_only,
            save_error: if read_only {
                "Draft recovery could not be read. The original file is preserved."
            } else {
                ""
            },
            favorite_undo: None,
            local: None,
            local_error: None,
            prepared_pass: None,
            last_pass: None,
        }
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn saved_draft_matches(&self, text: &str) -> bool {
        !self.read_only && self.save_error.is_empty() && !text.trim().is_empty()
            && matches_editor_text(self.state.active.text(), text)
    }

    pub fn context(&self) -> u64 {
        self.state.context
    }

    pub fn has_unsaved_recovery(&self) -> bool {
        self.state
            .retained
            .iter()
            .any(|r| r.id > self.saved_sequence)
    }

    pub fn busy(&self) -> bool {
        self.local.is_some()
    }

    fn prepared_pass(&self) -> Option<&PreparedPass> {
        self.prepared_pass.as_ref().filter(|pass| pass.operation.current(&self.state))
    }

    pub fn cancel_local(&mut self, id: u64) {
        if self.local.as_ref().is_some_and(|op| op.id == id) {
            self.local = None;
            self.version += 1;
        }
    }

    pub fn retained_text(&self, id: u64) -> Option<&str> {
        self.state
            .retained
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.text.as_str())
    }

    pub fn begin_local(
        &mut self,
        name: &str,
        args: &[Value],
        model: &str,
        prompt_target: PromptTarget,
    ) -> Option<LocalRequest> {
        if self.read_only || self.busy() || self.prepared_pass().is_some() || model.trim().is_empty() {
            return None;
        }
        let draft = &self.state.active;
        let string = |i| args.get(i).and_then(Value::as_str);
        let number = |i| args.get(i).and_then(Value::as_u64);
        let text = draft.text();
        let checked_text = !text.trim().is_empty() && number(0) == Some(self.context())
            && string(1).is_some_and(|editor| matches_editor_text(text, editor));
        let (kind, stage, target, text, system, user_prompt, json_output) = match name {
            "submit" if draft.pending.is_none() && number(1) == Some(self.context()) => {
                let instruction = string(0)?.trim();
                if instruction.is_empty() || instruction != draft.composer.trim() || instruction.len() > MAX_TEXT_BYTES {
                    return None;
                }
                let target = cleanup::resolve_prompt_target(prompt_target, text);
                (
                    LocalKind::Suggest {
                        kind: "improve",
                        clear_instruction: true,
                    },
                    StageKind::Submit,
                    target,
                    text.to_owned(),
                    cleanup::revise_system().to_owned(),
                    format!("{}\n<edit_request>\n{instruction}\n</edit_request>", wrap_draft(text)),
                    false,
                )
            }
            "feedback" if checked_text => {
                let target = cleanup::resolve_prompt_target(prompt_target, text);
                (
                    LocalKind::Feedback { suggest: false },
                    StageKind::Assess,
                    target,
                    text.to_owned(),
                    cleanup::assess_system(target).to_owned(),
                    wrap_draft(text),
                    true,
                )
            }
            "improve" if draft.pending.is_none() && !text.trim().is_empty() => {
                let target = cleanup::resolve_prompt_target(prompt_target, text);
                (
                    LocalKind::Feedback { suggest: true },
                    StageKind::Assess,
                    target,
                    text.to_owned(),
                    cleanup::assess_system(target).to_owned(),
                    wrap_draft(text),
                    true,
                )
            }
            "draft_pass" if checked_text && draft.pending.is_none() => {
                let refine = args.get(2)?.as_bool()?;
                let scored_revision = draft.selected.and_then(|i| draft.revisions.get(i));
                if refine
                    && !scored_revision
                        .is_some_and(|revision| revision.score.is_some() && !revision.stale)
                {
                    return None;
                }
                let target = cleanup::resolve_prompt_target(prompt_target, text);
                let (stage, system, user_prompt) = if refine {
                    let feedback = scored_revision
                        .and_then(|rev| rev.score.as_ref())
                        .map(format_refine_feedback)
                        .unwrap_or_default();
                    let prompt = if feedback.is_empty() {
                        wrap_draft(text)
                    } else {
                        format!("{}\n<feedback>\n{feedback}\n</feedback>", wrap_draft(text))
                    };
                    (StageKind::Refine, cleanup::refine_system(target).to_owned(), prompt)
                } else {
                    (StageKind::Engineer, cleanup::engineer_system(target).to_owned(), wrap_draft(text))
                };
                (
                    LocalKind::Pass { refine },
                    stage,
                    target,
                    text.to_owned(),
                    system,
                    user_prompt,
                    false,
                )
            }
            "rebuild" if draft.pending.is_none() && !text.trim().is_empty() => {
                let target = cleanup::resolve_prompt_target(prompt_target, text);
                (
                    LocalKind::Suggest {
                        kind: "rebuild",
                        clear_instruction: false,
                    },
                    StageKind::Rebuild,
                    target,
                    text.to_owned(),
                    cleanup::rebuild_system(target).to_owned(),
                    wrap_draft(text),
                    false,
                )
            }
            "review_candidate" => {
                let pending = draft.pending.as_ref()?;
                let candidate = number(0)?;
                let version = usize::try_from(number(1)?).ok()?;
                if pending.id != candidate || pending.stale(&self.state) {
                    return None;
                }
                let candidate_text = pending.versions.get(version)?.text.clone();
                let target = cleanup::resolve_prompt_target(prompt_target, &candidate_text);
                (
                    LocalKind::Review { candidate, version },
                    StageKind::Assess,
                    target,
                    candidate_text.clone(),
                    cleanup::assess_system(target).to_owned(),
                    wrap_draft(&candidate_text),
                    true,
                )
            }
            _ => return None,
        };
        if text.len() > MAX_TEXT_BYTES {
            return None;
        }
        let id = self.state.next_id();
        self.local = Some(LocalOperation {
            id,
            kind,
            target,
            context: self.context(),
            edit: self.state.edit_id,
            instruction: self.state.composer_edit_id,
            model: model.to_owned(),
            proposal: None,
        });
        self.prepared_pass = None;
        self.last_pass = None;
        self.local_error = None;
        self.version += 1;
        Some(LocalRequest {
            id,
            stage,
            system,
            user_prompt,
            text,
            json_output,
        })
    }

    pub fn finish_local(&mut self, id: u64, output: Result<String, String>, by: &str) -> Option<LocalRequest> {
        if self.local.as_ref().is_none_or(|op| op.id != id) {
            return None;
        }
        let mut op = self.local.take().unwrap();
        self.version += 1;
        if !op.current(&self.state) {
            return None;
        }
        let operation = op.kind.label();
        let context = op.context;
        let fail = |this: &mut Self, message: &str| {
            this.local_error = Some((context, operation, message.into()));
        };
        let output = match output {
            Ok(text) if !text.trim().is_empty() && text.len() <= MAX_TEXT_BYTES => text,
            Ok(_) => {
                fail(self, "The selected model did not return a usable result. Your draft and instruction are preserved.");
                return None;
            }
            Err(error) => {
                fail(self, &error);
                return None;
            }
        };
        let mut next = self.state.clone();
        match op.kind {
            LocalKind::Suggest {
                kind,
                clear_instruction,
            } => {
                if next.active.pending.is_some() {
                    return None;
                }
                let output = strip_echoed_draft_tags(&output, next.active.text());
                if output.is_empty() || output == next.active.text().trim() {
                    fail(self, "The writing model returned the same text. Your draft and instruction are preserved.");
                    return None;
                }
                let version = SuggestedVersion {
                    text: output.clone(),
                    by: op.model,
                    score: None,
                    based_on: None,
                    basis_score: None,
                };
                next.active.pending = Some(Candidate {
                    id,
                    source_context: context,
                    source_edit: next.edit_id,
                    source_text: next.active.text().to_owned(),
                    kind: kind.into(),
                    versions: vec![version],
                });
                if clear_instruction {
                    next.active.composer.clear();
                }
                if !self.commit(next) {
                    self.retain(&output, "The suggestion could not be saved in Prompt Lab. Copy it from recovery if you want to keep it.");
                    fail(self, "The suggestion could not be saved. Your draft and instruction are preserved.");
                }
            }
            LocalKind::Pass { refine } => {
                let revision = &next.active.revisions[next.active.selected?];
                let previous = revision.score.as_ref().filter(|_| refine).map(|score| score.score);
                if let Some(text) = op.proposal.take() {
                    let Some(score) = Assessment::parse(&output, by.to_owned(), id) else {
                        fail(self, "The checking model did not return a usable score. Your draft is preserved.");
                        return None;
                    };
                    if previous.is_none_or(|previous| score.score > previous) {
                        self.prepared_pass = Some(PreparedPass {
                            revision: Revision {
                                text,
                                kind: if refine { "refine" } else { "engineer" }.into(),
                                by: Some(op.model.clone()),
                                score: Some(score),
                                stale: false,
                            },
                            operation: op,
                        });
                        return None;
                    }
                } else {
                    let text = strip_echoed_draft_tags(&output, &revision.text)
                        .replace("\r\n", "\n")
                        .replace('\r', "\n");
                    if !text.is_empty() && (!refine || text != revision.text.trim()) {
                        let target = op.target;
                        op.proposal = Some(text.clone());
                        self.local = Some(op);
                        return Some(LocalRequest {
                            id,
                            stage: StageKind::Assess,
                            system: cleanup::assess_system(target).to_owned(),
                            user_prompt: wrap_draft(&text),
                            text,
                            json_output: true,
                        });
                    }
                }
                self.last_pass = Some(DraftPassResult {
                    id, source: context, context,
                    message: "No meaningful improvement this pass. Your draft and score are unchanged.",
                });
            }
            LocalKind::Feedback { .. } | LocalKind::Review { .. } => {
                let Some(assessment) = Assessment::parse(&output, op.model, id) else {
                    fail(
                        self,
                        "The checking model did not return usable feedback. Your draft is preserved.",
                    );
                    return None;
                };
                match op.kind {
                    LocalKind::Feedback { suggest } => {
                        let Some(index) = next.active.selected else {
                            return None;
                        };
                        let revision = &mut next.active.revisions[index];
                        revision.score = Some(assessment.clone());
                        revision.stale = false;
                        let has_suggestion = next.active.pending.is_some()
                            || Self::suggest_from_assessment(&mut next, id, &assessment);
                        if suggest && !has_suggestion {
                            fail(self, "The checking model returned no changed draft. Your feedback is available.");
                        }
                    }
                    LocalKind::Review { candidate, version } => {
                        let Some(pending) =
                            next.active.pending.as_mut().filter(|c| c.id == candidate)
                        else {
                            return None;
                        };
                        let Some(source) = pending.versions.get_mut(version) else {
                            return None;
                        };
                        source.score = Some(assessment.clone());
                        if !assessment.improved.trim().is_empty()
                            && assessment.improved.trim() != source.text.trim()
                        {
                            pending.versions.push(SuggestedVersion {
                                text: assessment.improved.clone(),
                                by: assessment.by.clone(),
                                score: None,
                                based_on: Some(version),
                                basis_score: Some(assessment),
                            });
                        }
                    }
                    _ => unreachable!(),
                }
                if !self.commit(next) {
                    fail(
                        self,
                        "The feedback could not be saved. Your draft is preserved.",
                    );
                }
            }
        }
        None
    }

    fn suggest_from_assessment(state: &mut State, id: u64, assessment: &Assessment) -> bool {
        let text = assessment.improved.trim();
        let source = state.active.text();
        if text.is_empty() || text == source.trim() {
            return false;
        }
        state.active.pending = Some(Candidate {
            id, source_context: state.context, source_edit: state.edit_id, source_text: source.into(),
            kind: "improve".into(),
            versions: vec![SuggestedVersion {
                text: text.into(), by: assessment.by.clone(), score: None, based_on: None, basis_score: None,
            }],
        });
        true
    }

    pub fn reuse_feedback_for_improvement(&mut self) -> Option<bool> {
        if self.read_only || self.busy() || self.prepared_pass().is_some() || self.state.active.pending.is_some() {
            return None;
        }
        let revision = self.state.active.revisions.get(self.state.active.selected?)?;
        let assessment = revision.score.as_ref().filter(|_| !revision.stale && !revision.text.trim().is_empty())?;
        let mut next = self.state.clone();
        let id = next.next_id();
        let context = self.context();
        if !Self::suggest_from_assessment(&mut next, id, assessment) {
            self.local_error = Some((context, "scoring", "The checking model returned no changed draft. Your feedback is available.".into()));
            self.version += 1;
            return Some(true);
        }
        self.local_error = None;
        Some(self.commit(next))
    }

    fn finish_draft_pass(&mut self, context: Option<u64>, id: Option<u64>) -> Value {
        if self.prepared_pass().is_some_and(|pass| {
            context == Some(pass.operation.context) && id == Some(pass.operation.id)
        }) {
            let pass = self.prepared_pass.take().unwrap();
            let mut next = self.state.clone();
            next.active.revisions.push(pass.revision);
            next.active.selected = Some(next.active.revisions.len() - 1);
            next.change_context();
            if self.commit(next) {
                self.last_pass = Some(DraftPassResult {
                    id: pass.operation.id,
                    source: pass.operation.context,
                    context: self.context(),
                    message: "Draft updated.",
                });
            } else {
                self.local_error = Some((pass.operation.context, pass.operation.kind.label(),
                    "The scored draft could not be saved. Your draft and score were kept.".into()));
            }
        }
        self.snapshot()
    }

    fn commit(&mut self, next: State) -> bool {
        if self.read_only {
            return false;
        }
        let saved = serde_json::to_vec(&next)
            .map_err(io::Error::other)
            .and_then(|bytes| storage::atomic_write(&self.path, &bytes));
        self.version += 1;
        if saved.is_err() {
            self.save_error = "Draft recovery could not be saved. Try again before closing Listen.";
            return false;
        }
        self.saved_sequence = next.sequence;
        self.state = next;
        if self.prepared_pass().is_none() {
            self.prepared_pass = None;
        }
        self.save_error = "";
        true
    }

    pub fn retain(&mut self, text: &str, reason: &str) -> bool {
        if text.trim().is_empty() {
            return false;
        }
        let mut next = self.state.clone();
        next.retain(text, reason);
        if self.commit(next.clone()) {
            return true;
        }
        self.state = next;
        self.version += 1;
        false
    }

    pub fn accept_dictation(&mut self, text: &str, context: u64) -> bool {
        if text.trim().is_empty() {
            return false;
        }
        if context != self.context() {
            self.retain(text, "The active draft changed before this dictation finished. Copy it into the draft you want.");
            return false;
        }
        let mut next = self.state.clone();
        let previous = next.active.text().trim_end();
        let combined = if previous.is_empty() {
            text.to_owned()
        } else {
            format!("{previous}\n\n{text}")
        };
        if combined.len() <= MAX_TEXT_BYTES {
            next.active.add_revision(combined, "typed");
            next.change_context();
            if self.commit(next) {
                return true;
            }
        }
        self.retain(
            text,
            "The dictation could not be saved to the draft. Copy it where you want it.",
        );
        false
    }

    fn snapshot(&self) -> Value {
        let state = &self.state;
        let current = state
            .active
            .selected
            .and_then(|i| state.active.revisions.get(i));
        let prev_score = state.active.selected.and_then(|i| {
            state.active.revisions[..i]
                .iter()
                .rev()
                .find_map(|r| r.score.as_ref().map(|score| score.score))
        });
        let pending = state.active.pending.as_ref();
        let pass = self.last_pass.as_ref().filter(|pass| pass.context == state.context);
        let (operation, error) = self
            .local_error
            .as_ref().filter(|(context, _, _)| *context == state.context)
            .map_or(("", ""), |(_, operation, error)| (*operation, error.as_str()));
        let revisions: Vec<_> = state
            .active
            .revisions
            .iter()
            .enumerate()
            .map(|(i, r)| {
                json!({"i": i, "kind": r.kind,
                "score": r.score.as_ref().map(|s| s.score), "by": r.by, "stale": r.stale})
            })
            .collect();
        json!({
            "workspace_id": state.active.id, "draft_id": state.context,
            "edit_id": state.edit_id, "composer_edit_id": state.composer_edit_id,
            "text": state.active.text(), "revs": revisions,
            "cur": state.active.selected.map_or(-1, |i| i as i64),
            "composer": state.active.composer, "fav": state.active.favorite,
            "saved": self.save_error.is_empty(), "save_error": self.save_error,
            "score": current.and_then(|r| r.score.as_ref()), "prev_score": prev_score,
            "stale": current.is_some_and(|r| r.stale), "by": current.and_then(|r| r.by.as_deref()),
            "pending": pending.map(|candidate| json!({
                "id": candidate.id, "source_text": candidate.source_text, "kind": candidate.kind,
                "versions": candidate.versions, "stale": candidate.stale(state),
            })),
            "prepared_pass": self.prepared_pass().map(|pass| pass.operation.id),
            "draft_pass": pass.map(|pass| json!({
                "id": pass.id, "draft_id": pass.context, "source_id": pass.source,
                "message": pass.message,
            })),
            "busy": self.local.as_ref().map_or("", |op| op.kind.label()),
            "error": error, "error_op": operation, "session_cost": 0, "session_unpriced": 0,
        })
    }

    fn save_edit(&mut self, args: &[Value], composer: bool) -> bool {
        let Some(text) = args
            .first()
            .and_then(Value::as_str)
            .filter(|s| s.len() <= MAX_TEXT_BYTES)
        else {
            return false;
        };
        let Some(context) = args.get(1).and_then(Value::as_u64) else {
            return false;
        };
        let Some(edit_id) = args.get(2).and_then(Value::as_i64).filter(|id| *id >= 0) else {
            return false;
        };
        if context != self.context() {
            return false;
        }
        let (last_edit, current) = if composer {
            (
                self.state.composer_edit_id,
                self.state.active.composer.as_str(),
            )
        } else {
            (self.state.edit_id, self.state.active.text())
        };
        if edit_id < last_edit || (edit_id == last_edit && current != text) {
            return false;
        }
        if edit_id == last_edit && self.save_error.is_empty() {
            return true;
        }
        let mut next = self.state.clone();
        if composer {
            next.active.composer = text.to_owned();
            next.composer_edit_id = edit_id;
        } else {
            if let Some(i) = next.active.selected {
                let revision = &mut next.active.revisions[i];
                if revision.text != text && revision.score.is_some() {
                    revision.stale = true;
                }
                revision.text = text.to_owned();
            } else if !text.trim().is_empty() {
                next.active.add_revision(text.to_owned(), "typed");
            }
            next.edit_id = edit_id;
        }
        self.commit(next)
    }

    fn save_favorite(
        &mut self,
        name: Option<&str>,
        text: Option<&str>,
        replace: Option<bool>,
    ) -> &'static str {
        let Some(name) = name
            .map(str::trim)
            .filter(|s| !s.is_empty() && s.len() <= 512)
        else {
            return "error";
        };
        let Some(text) = text.filter(|s| !s.trim().is_empty() && s.len() <= MAX_TEXT_BYTES) else {
            return "error";
        };
        let Some(replace) = replace else {
            return "error";
        };
        let mut next = self.state.clone();
        if let Some(i) = next.favorites.iter().position(|f| f.name == name) {
            if next.favorites[i].text != text {
                if !replace {
                    return "conflict";
                }
                let mut previous = Draft::new(next.next_id());
                previous.add_revision(next.favorites[i].text.clone(), "favorite");
                next.archived.push(previous);
                next.favorites[i].text = text.to_owned();
            }
        } else {
            next.favorites.push(Favorite {
                name: name.to_owned(),
                text: text.to_owned(),
                when: storage::now_stamp().0,
                uses: 0,
            });
        }
        next.active.favorite = Some(name.to_owned());
        if self.commit(next) {
            "saved"
        } else {
            "error"
        }
    }

    pub fn call(&mut self, name: &str, args: &[Value]) -> Option<Value> {
        let string = |i| args.get(i).and_then(Value::as_str);
        let number = |i| args.get(i).and_then(Value::as_u64);
        Some(match name {
            "get_lab" => self.snapshot(),
            "finish_draft_pass" => self.finish_draft_pass(number(0), number(1)),
            "set_text" => json!(self.save_edit(args, false)),
            "set_composer" => json!(self.save_edit(args, true)),
            "select_rev" => {
                let Some(index) = number(0)
                    .and_then(|n| usize::try_from(n).ok())
                    .filter(|i| *i < self.state.active.revisions.len())
                else {
                    return Some(json!(false));
                };
                let mut next = self.state.clone();
                next.active.selected = Some(index);
                next.change_context();
                json!(self.commit(next))
            }
            "new_draft" => {
                let mut next = self.state.clone();
                let draft = Draft::new(next.next_id());
                next.switch_draft(draft);
                json!(self.commit(next))
            }
            "list_drafts" => {
                let rows: Vec<_> = self
                    .state
                    .archived
                    .iter()
                    .map(|draft| json!({"id": draft.id.to_string(), "title": draft.title()}))
                    .collect();
                json!(rows)
            }
            "restore_draft" | "delete_draft" => {
                if name == "delete_draft" && string(1) != Some("DELETE") {
                    return Some(json!(false));
                }
                let Some(id) = string(0).and_then(|s| s.parse::<u64>().ok()) else {
                    return Some(json!(false));
                };
                let Some(index) = self.state.archived.iter().position(|d| d.id == id) else {
                    return Some(json!(false));
                };
                let mut next = self.state.clone();
                let draft = next.archived.remove(index);
                if name == "restore_draft" {
                    next.switch_draft(draft);
                }
                json!(self.commit(next))
            }
            "get_favorites" => {
                let mut favorites: Vec<_> = self.state.favorites.iter().collect();
                favorites.sort_by_key(|f| (std::cmp::Reverse(f.uses), f.name.to_lowercase()));
                json!(favorites)
            }
            "save_favorite_checked" => {
                json!({"status": self.save_favorite(string(0), string(1),args.get(2).and_then(Value::as_bool))})
            }
            "load_favorite" => {
                let Some(favorite) = self
                    .state
                    .favorites
                    .iter()
                    .find(|f| Some(f.name.as_str()) == string(0))
                else {
                    return Some(json!(false));
                };
                let mut next = self.state.clone();
                let mut draft = Draft::new(next.next_id());
                draft.favorite = Some(favorite.name.clone());
                draft.add_revision(favorite.text.clone(), "favorite");
                next.switch_draft(draft);
                json!(self.commit(next))
            }
            "remove_favorite" => {
                let Some(index) = self
                    .state
                    .favorites
                    .iter()
                    .position(|f| Some(f.name.as_str()) == string(0))
                else {
                    return Some(Value::Null);
                };
                let mut next = self.state.clone();
                let token = next.next_id().to_string();
                let removed = next.favorites.remove(index);
                if next.active.favorite.as_deref() == Some(&removed.name) {
                    next.active.favorite = None;
                }
                if !self.commit(next) {
                    return Some(Value::Null);
                }
                self.favorite_undo = Some((token.clone(), removed));
                json!(token)
            }
            "undo_favorite_removal" => {
                let Some((token, favorite)) = &self.favorite_undo else {
                    return Some(json!(false));
                };
                if string(0) != Some(token)
                    || self.state.favorites.iter().any(|f| f.name == favorite.name)
                {
                    return Some(json!(false));
                }
                let mut next = self.state.clone();
                next.favorites.push(favorite.clone());
                let saved = self.commit(next);
                if saved {
                    self.favorite_undo = None;
                }
                json!(saved)
            }
            "get_retained" => {
                let mut rows = self.state.retained.clone();
                for row in &mut rows {
                    if row.id > self.saved_sequence {
                        row.message.push_str(" This copy is open in Listen and could not be saved. Copy it before closing.");
                    }
                }
                json!(rows)
            }
            "retain_editor" => {
                let Some(text) = string(0).filter(|s| s.len() <= MAX_TEXT_BYTES) else {
                    return Some(json!(false));
                };
                let reason = "The active revision changed. Your edit is available to copy here.";
                if let Some(row) = self
                    .state
                    .retained
                    .iter()
                    .find(|r| r.text == text && r.message == reason)
                {
                    json!(row.id <= self.saved_sequence || self.commit(self.state.clone()))
                } else {
                    json!(self.retain(text, reason))
                }
            }
            "dismiss_retained" => {
                let Some(index) = self
                    .state
                    .retained
                    .iter()
                    .position(|row| Some(row.id) == number(0))
                else {
                    return Some(json!(false));
                };
                let mut next = self.state.clone();
                let row = next.retained.remove(index);
                if row.id > self.saved_sequence {
                    self.state = next;
                    self.version += 1;
                    json!(true)
                } else {
                    json!(self.commit(next))
                }
            }
            "clear_private_data" if string(0) == Some("workspace") => {
                if string(1) != Some("DELETE") {
                    return Some(json!({"ok": false, "message": "Canceled. Nothing was changed."}));
                }
                let mut next = self.state.clone();
                next.active = Draft::new(next.next_id());
                next.archived.clear();
                next.retained.clear();
                next.change_context();
                let saved = self.commit(next);
                json!({"ok": saved, "message": if saved { "Drafts and recovery text cleared." } else { "The change could not be saved. Try again." }})
            }
            "apply_candidate" | "discard_candidate" => {
                let Some(candidate) = self.state.active.pending.as_ref() else {
                    return Some(json!(false));
                };
                if Some(candidate.id) != number(0) || self.busy() {
                    return Some(json!(false));
                }
                let mut next = self.state.clone();
                if name == "apply_candidate" {
                    if candidate.stale(&self.state) {
                        return Some(json!(false));
                    }
                    let version = number(1).and_then(|i| usize::try_from(i).ok());
                    let Some(version) = version.and_then(|i| candidate.versions.get(i)) else {
                        return Some(json!(false));
                    };
                    next.active
                        .add_revision(version.text.clone(), &candidate.kind);
                    let revision = next.active.revisions.last_mut().unwrap();
                    revision.by = Some(version.by.clone());
                    revision.score = version.score.clone();
                    next.change_context();
                }
                next.active.pending = None;
                let saved = self.commit(next);
                if saved {
                    self.local_error = None;
                    self.last_pass = None;
                }
                json!(saved)
            }
            "lab_kept" => json!(true),
            _ => return None,
        })
    }
}

fn migrate_legacy(profile: &Path) -> io::Result<State> {
    let mut state = State::default();
    match fs::read_to_string(profile.join("draft.txt")) {
        Ok(text) if !text.is_empty() => state.active.add_revision(text, "typed"),
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    match fs::read(profile.join("favorites.json")) {
        Ok(bytes) => state.favorites = serde_json::from_slice(&bytes).map_err(io::Error::other)?,
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(label: &str) -> PathBuf {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "listen-rs-workspace-{label}-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    fn clean_profile(path: &Path) {
        for name in ["workspace.json", "draft.txt"] {
            match fs::remove_file(path.join(name)) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => panic!("could not remove {name}: {error}"),
            }
        }
        fs::remove_dir(path).unwrap();
    }

    #[test]
    fn restores_editor_instruction_and_revisions_without_accepting_stale_edits() {
        let path = profile("restore");
        let mut w = Workspace::load(&path);
        let first = w.context();
        assert_eq!(
            w.call(
                "set_text",
                &[json!("First revision"), json!(first), json!(1)]
            ),
            Some(json!(true))
        );
        assert_eq!(
            w.call(
                "set_composer",
                &[json!("Keep the name"), json!(first), json!(1)]
            ),
            Some(json!(true))
        );
        assert_eq!(
            w.call(
                "set_text",
                &[json!("same-number overwrite"), json!(first), json!(1)]
            ),
            Some(json!(false))
        );
        assert!(w.accept_dictation("Second revision", first));
        assert_eq!(w.call("new_draft", &[]), Some(json!(true)));
        assert!(!w.accept_dictation("Late dictation", first));
        assert_eq!(
            w.call("set_text", &[json!("stale text"), json!(first), json!(3)]),
            Some(json!(false))
        );

        let mut reopened = Workspace::load(&path);
        let archives = reopened.call("list_drafts", &[]).unwrap();
        assert_eq!(archives[0]["title"], "First revision");
        assert_eq!(
            reopened.call("restore_draft", &[archives[0]["id"].clone()]),
            Some(json!(true))
        );
        assert_eq!(reopened.snapshot()["composer"], "Keep the name");
        assert_eq!(
            reopened.snapshot()["text"],
            "First revision\n\nSecond revision"
        );
        assert_eq!(reopened.call("select_rev", &[json!(0)]), Some(json!(true)));
        assert_eq!(reopened.snapshot()["text"], "First revision");
        assert_eq!(
            reopened.call("get_retained", &[]).unwrap()[0]["text"],
            "Late dictation"
        );
        clean_profile(&path);
    }

    #[test]
    fn preserves_legacy_data_and_rolls_back_failed_commits() {
        let path = profile("rollback");
        fs::write(path.join("draft.txt"), "Legacy text").unwrap();
        let mut w = Workspace::load(&path);
        assert_eq!(w.snapshot()["text"], "Legacy text");
        let context = w.context();
        let real_path = w.path.clone();
        w.path = path.join("missing").join("workspace.json");
        assert_eq!(w.call("new_draft", &[]), Some(json!(false)));
        assert_eq!(w.context(), context);
        assert_eq!(w.snapshot()["text"], "Legacy text");
        let retry = [json!("An editor retry")];
        assert_eq!(w.call("retain_editor", &retry), Some(json!(false)));
        assert_eq!(w.call("retain_editor", &retry), Some(json!(false)));
        let retained = w.call("get_retained", &[]).unwrap();
        assert_eq!(retained.as_array().unwrap().len(), 1);
        assert_eq!(
            w.call("dismiss_retained", &[retained[0]["id"].clone()]),
            Some(json!(true))
        );
        assert!(!w.has_unsaved_recovery());
        assert!(!w.retain("Still visible", "A result"));
        assert!(w.has_unsaved_recovery());
        w.path = real_path;
        assert_eq!(
            w.call("set_text", &[json!("New text"), json!(context), json!(1)]),
            Some(json!(true))
        );
        assert!(!w.has_unsaved_recovery());

        let mut w = Workspace::load(&path);
        assert_eq!(w.snapshot()["text"], "New text");
        let retained = w.call("get_retained", &[]).unwrap();
        assert_eq!(retained[0]["text"], "Still visible");
        assert_eq!(retained[0]["message"], "A result");
        assert_eq!(
            fs::read_to_string(path.join("draft.txt")).unwrap(),
            "Legacy text"
        );
        clean_profile(&path);
    }

    #[test]
    fn favorite_replace_and_undo_keep_authored_text() {
        let path = profile("favorite");
        let mut w = Workspace::load(&path);
        assert_eq!(
            w.save_favorite(Some("Example"), Some("First"), Some(false)),
            "saved"
        );
        assert_eq!(
            w.save_favorite(Some("Example"), Some("Second"), Some(false)),
            "conflict"
        );
        assert_eq!(
            w.save_favorite(Some("Example"), Some("Second"), Some(true)),
            "saved"
        );
        assert_eq!(w.call("list_drafts", &[]).unwrap()[0]["title"], "First");
        let token = w.call("remove_favorite", &[json!("Example")]).unwrap();
        assert_eq!(w.call("get_favorites", &[]), Some(json!([])));
        assert_eq!(w.call("undo_favorite_removal", &[token]), Some(json!(true)));
        assert_eq!(w.call("get_favorites", &[]).unwrap()[0]["text"], "Second");
        fs::write(path.join("workspace.json"), "broken data").unwrap();
        let mut corrupt = Workspace::load(&path);
        assert_eq!(corrupt.call("new_draft", &[]), Some(json!(false)));
        assert_eq!(
            fs::read_to_string(path.join("workspace.json")).unwrap(),
            "broken data"
        );
        clean_profile(&path);
    }

    fn assessment(improved: &str) -> String {
        json!({"score": 8, "criteria": {"clarity": 5, "context": 4, "task": 4, "constraints": 3, "success_criteria": 4, "format": 4},
            "gaps": ["Add a verification step"], "improved": improved}).to_string()
    }

    #[test]
    fn engineer_keeps_draft_until_written_text_is_scored_and_published() {
        let path = profile("scored-pass");
        let mut w = Workspace::load(&path);
        let context = w.context();
        assert!(w.save_edit(&[json!("Rough draft"), json!(context), json!(1)], false));
        let args = [json!(context), json!("Rough draft"), json!(false)];
        let request = w.begin_local("draft_pass", &args, "Writer", PromptTarget::Auto).unwrap();
        assert_eq!(request.stage, StageKind::Engineer);
        assert!(!request.json_output);
        assert!(request.system.contains("NEVER answer, execute"));
        assert!(request.user_prompt.contains("<draft>\nRough draft\n</draft>"));
        let judge = w.finish_local(request.id, Ok("Engineered draft".into()), "Writer").unwrap();
        assert_eq!(judge.stage, StageKind::Assess);
        assert!(judge.json_output);
        assert_eq!(judge.text, "Engineered draft");
        assert!(judge.system.contains("Calibration:"));
        let state = w.snapshot();
        assert!(state["pending"].is_null(), "Engineer should score its output instead of opening Compare");
        assert!(state["score"].is_null());
        assert_eq!(state["text"], "Rough draft");
        assert!(w.begin_local("draft_pass", &args, "Writer", PromptTarget::Auto).is_none());
        assert!(w.finish_local(judge.id, Ok(assessment("")), "Checker").is_none());
        assert_eq!(w.snapshot()["prepared_pass"], request.id);
        assert_eq!(w.finish_draft_pass(Some(context + 1), Some(request.id))["text"], "Rough draft");
        let engineered = w.finish_draft_pass(Some(context), Some(request.id));
        assert_eq!(engineered["text"], "Engineered draft");
        assert_eq!(engineered["by"], "Writer");
        assert_eq!(engineered["score"]["by"], "Checker");
        assert_eq!(engineered["draft_pass"]["source_id"], context);
        assert_ne!(engineered["draft_id"], context);

        let current = w.context();
        let args = [json!(current), json!("Engineered draft"), json!(true)];
        let run = w.begin_local("draft_pass", &args, "Writer", PromptTarget::Auto).unwrap();
        assert_eq!(run.stage, StageKind::Refine);
        assert!(!run.json_output);
        assert!(run.user_prompt.contains("<feedback>"), "Refine must pass checker feedback");
        assert!(run.user_prompt.contains("Add a verification step"), "Refine must include exact checker gaps");
        assert!(run.user_prompt.contains("constraints (3/5)"), "Refine must list weak criteria");
        assert!(w.finish_local(run.id, Ok("Equally good draft".into()), "Writer").is_some());
        assert!(w.finish_local(run.id, Ok(assessment("")), "Checker").is_none());
        assert_eq!(w.snapshot()["draft_pass"]["id"], run.id);
        assert!(w.snapshot()["prepared_pass"].is_null());
        assert_eq!(w.snapshot()["text"], "Engineered draft");
        assert!(w.saved_draft_matches("Engineered draft"));

        let run = w.begin_local("draft_pass", &args, "Writer", PromptTarget::ClaudeCode).unwrap();
        assert!(run.system.contains("coding-agent"));
        let follow = w.finish_local(run.id, Ok("Better draft".into()), "Writer").unwrap();
        assert!(follow.system.contains("AI coding agent"), "Follow-up scorer must inherit resolved target");
        let higher = assessment("").replace("\"score\":8", "\"score\":9");
        assert!(w.finish_local(run.id, Ok(higher), "Checker").is_none());
        assert_eq!(w.finish_draft_pass(Some(current), Some(run.id))["text"], "Better draft");
        assert_eq!(w.snapshot()["revs"].as_array().unwrap().len(), 3);
        let reopened = Workspace::load(&path);
        assert_eq!(reopened.snapshot()["text"], "Better draft");
        assert_eq!(reopened.snapshot()["score"]["score"].as_f64(), Some(9.0));
        clean_profile(&path);
    }

    #[test]
    #[ignore = "uses an installed local Ollama model with a sample draft"]
    fn engineer_with_local_ollama() {
        use crate::{cleanup, storage::{PromptProvider, Settings}};
        let path = profile("ollama-pass");
        let mut w = Workspace::load(&path);
        let context = w.context();
        let source = "Write a three-item checklist for proofreading a short email. Use plain language.";
        let model = "qwen3.5:2b";
        let settings = Settings::default();
        let run = |request: &LocalRequest| cleanup::run_prompt_stage(
            PromptProvider::Local, model, &settings, &request.system, &request.user_prompt, request.json_output,
            || panic!("This fixture must stay local"),
        );
        assert!(w.save_edit(&[json!(source), json!(context), json!(1)], false));
        let writer = w.begin_local("draft_pass", &[json!(context), json!(source), json!(false)], model, settings.prompt_target).unwrap();
        let written = run(&writer);
        let judge = w.finish_local(writer.id, written, model);
        if let Some(judge) = judge {
            w.finish_local(judge.id, run(&judge), model);
        }
        let result = w.finish_draft_pass(Some(context), Some(writer.id));
        clean_profile(&path);
        assert!(result["score"]["score"].is_number(), "Local pass did not score: {}", result["error"]);
        assert_eq!(result["revs"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn local_suggestion_is_saved_for_review_and_applied_only_once() {
        let path = profile("local-operation");
        let mut w = Workspace::load(&path);
        let context = w.context();
        assert!(w.save_edit(&[json!("Original draft"), json!(context), json!(1)], false));
        assert!(w.save_edit(&[json!("Add a check"), json!(context), json!(1)], true));
        let request = w
            .begin_local(
                "submit",
                &[json!("Add a check"), json!(context)],
                "Test local",
                PromptTarget::Auto,
            )
            .unwrap();
        assert_eq!(request.stage, StageKind::Submit);
        assert_eq!(request.text, "Original draft");
        assert!(request.user_prompt.contains("<edit_request>\nAdd a check\n</edit_request>"));
        assert!(w.busy());
        w.finish_local(request.id, Ok("First suggestion".into()), "Test local");
        assert_eq!(w.snapshot()["text"], "Original draft");
        assert_eq!(w.snapshot()["composer"], "");

        let mut w = Workspace::load(&path);
        assert_eq!(
            w.snapshot()["pending"]["versions"][0]["text"],
            "First suggestion"
        );
        let review = w
            .begin_local(
                "review_candidate",
                &[json!(request.id), json!(0)],
                "Test local",
                PromptTarget::Auto,
            )
            .unwrap();
        assert!(review.json_output);
        let apply = [json!(request.id), json!(1)];
        assert_eq!(w.call("apply_candidate", &apply), Some(json!(false)));
        w.finish_local(review.id, Ok(assessment("Second suggestion")), "Test local");
        assert_eq!(
            w.snapshot()["pending"]["versions"][0]["score"]["score"].as_f64(),
            Some(8.0)
        );
        assert_eq!(w.call("apply_candidate", &apply), Some(json!(true)));
        assert_eq!(w.call("apply_candidate", &apply), Some(json!(false)));
        assert_eq!(w.snapshot()["text"], "Second suggestion");
        assert_eq!(w.snapshot()["revs"].as_array().unwrap().len(), 2);
        clean_profile(&path);
    }

    #[test]
    fn stale_completions_and_malformed_feedback_leave_drafts_unchanged() {
        let path = profile("stale-local");
        let mut w = Workspace::load(&path);
        let context = w.context();
        assert!(w.save_edit(&[json!("Original"), json!(context), json!(1)], false));
        let args = [json!(context), json!("Original"), json!(false)];
        let output = w.begin_local("draft_pass", &args, "Test local", PromptTarget::Auto).unwrap();
        assert!(w.save_edit(&[json!("New edit"), json!(context), json!(2)], false));
        assert!(w.finish_local(output.id, Ok("Stale answer".into()), "Test local").is_none());
        assert!(w.snapshot()["pending"].is_null());
        let args = [json!(context), json!("New edit")];
        let feedback = w.begin_local("feedback", &args, "Test local", PromptTarget::Auto).unwrap();
        assert!(feedback.json_output);
        w.finish_local(feedback.id, Ok("Malformatted".into()), "Test local");
        assert!(w.snapshot()["score"].is_null());
        assert_eq!(w.snapshot()["error_op"], "scoring");
        let feedback = w.begin_local("feedback", &args, "Test local", PromptTarget::Auto).unwrap();
        w.finish_local(feedback.id, Ok(assessment("Suggested improvement")), "Test local");
        assert_eq!(w.snapshot()["score"]["feedback_id"], feedback.id);
        assert_eq!(w.snapshot()["pending"]["versions"][0]["text"], "Suggested improvement");
        assert_eq!(w.call("discard_candidate", &[json!(feedback.id)]), Some(json!(true)));
        assert_eq!(w.reuse_feedback_for_improvement(), Some(true));
        let suggestion = w.snapshot()["pending"]["id"].clone();
        assert_eq!(w.snapshot()["pending"]["versions"][0]["text"], "Suggested improvement");
        assert_eq!(w.call("discard_candidate", &[suggestion]), Some(json!(true)));

        let output = w
            .begin_local(
                "draft_pass",
                &[json!(context), json!("New edit"), json!(false)],
                "Test local",
                PromptTarget::Auto,
            )
            .unwrap();
        assert!(w.finish_local(output.id, Ok("Earlier proposal".into()), "Writer").is_some());
        w.finish_local(output.id, Ok(assessment("")), "Checker");
        assert_eq!(w.snapshot()["prepared_pass"], output.id);
        assert!(w.save_edit(&[json!("Latest edit"), json!(context), json!(3)], false));
        assert_eq!(w.finish_draft_pass(Some(context), Some(output.id))["text"], "Latest edit");
        assert!(w.snapshot()["prepared_pass"].is_null());
        assert_eq!(w.snapshot()["text"], "Latest edit");
        clean_profile(&path);
    }

    #[test]
    fn assessment_parse_and_refine_feedback_handle_model_quirks() {
        // Null fields, uppercase JSON fences, leading prose, extra criteria key, echoed <draft> tags.
        let messy = "Here is the review:\n```JSON\n{\"score\": 9, \"criteria\": {\"clarity\": 5, \"context\": 4.5, \"task\": 5, \"constraints\": 5, \"success_criteria\": 5, \"format\": 5, \"overall\": 5}, \"gaps\": null, \"improved\": null}\n```";
        let parsed = Assessment::parse(messy, "Checker".into(), 7).expect("parse messy json");
        assert_eq!(parsed.score, 9.0);
        assert_eq!(parsed.criteria.len(), 6);
        assert!(parsed.gaps.is_empty());
        assert!(parsed.improved.is_empty());
        let fb = format_refine_feedback(&parsed);
        assert!(fb.contains("Room to strengthen: context (4.5/5)"), "got: {fb}");

        // All 5s with no gaps produces empty feedback so Refine omits <feedback>.
        let perfect = Assessment::parse(
            r#"{"score":10,"criteria":{"clarity":5,"context":5,"task":5,"constraints":5,"success_criteria":5,"format":5},"gaps":[],"improved":""}"#,
            "Checker".into(),
            8,
        ).unwrap();
        assert!(format_refine_feedback(&perfect).is_empty());

        // Echoed outer <draft> tags are stripped unless source itself started with <draft>.
        assert_eq!(strip_echoed_draft_tags("<draft>\nClean prompt\n</draft>", "Raw draft"), "Clean prompt");
        assert_eq!(strip_echoed_draft_tags("<draft>\nKept\n</draft>", "<draft>original</draft>"), "<draft>\nKept\n</draft>");

        // Scale-confused judge returning 1..5 average (4.0) when criteria avg 3.83/5 normalizes to 8.0/10,
        // while a genuine weak 4/10 (criteria avg 2.0/5) remains 4.0/10.
        let confused = Assessment::parse(
            r#"{"score":4,"criteria":{"clarity":4,"context":3,"task":4,"constraints":5,"success_criteria":3,"format":4},"gaps":["Tighten criteria"],"improved":""}"#,
            "gemini-3.5-flash-lite".into(),
            9,
        ).unwrap();
        assert_eq!(confused.score, 8.0);

        let genuinely_low = Assessment::parse(
            r#"{"score":4,"criteria":{"clarity":2,"context":2,"task":2,"constraints":2,"success_criteria":2,"format":2},"gaps":["Too vague"],"improved":""}"#,
            "gemini-3.5-flash-lite".into(),
            10,
        ).unwrap();
        assert_eq!(genuinely_low.score, 4.0);
    }
}
