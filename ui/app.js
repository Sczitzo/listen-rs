/* Listen window — vanilla JS over the pywebview bridge. Polls state every 250 ms (33 ms while recording, 1 s while the window is hidden); delivery status at most once a second and never while recording. */
(() => {
const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const originalPicture = window.listenDashboard?.originalPicture === true;
let pictureKeySide, pictureStyle, pictureCleanup, pictureNavStatus;
if (originalPicture) {
  document.documentElement.dataset.originalPicture = "";
  const picture = $("#original-dictate").content.cloneNode(true);
  const header = $(".dictate-head", picture);
  const title = $('.page[data-page="dictate"] h1');
  title.classList.remove("sr-only");
  header.firstElementChild.prepend(title);
  header.append($("#status-dictate"));
  $(".recording-summary").replaceWith($("#hero-title"));
  $(".dictate-center").before(header);
  $(".vocabulary").before($(".dictate-style", picture));
  const recent = $(".recent", picture);
  $("legend", recent).append($(".recent-history"));
  recent.append($("#recent-list"));
  $(".recent-wrap").replaceChildren(recent);
  const keyCard = $("#btn-hotkey-card");
  keyCard.firstElementChild.textContent = "In Listen";
  pictureKeySide = document.createElement("span");
  pictureKeySide.id = "hero-key-side";
  pictureKeySide.setAttribute("aria-hidden", "true");
  pictureKeySide.textContent = "right key";
  keyCard.append(pictureKeySide);
  pictureStyle = $("#dictate-style");
  pictureCleanup = $("#dictate-cleanup");
  pictureNavStatus = $(".nav-dot", picture);
  $(".rail").append(pictureNavStatus);
}
const esc = s => String(s ?? "").replace(/[&<>"]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
const DEMO_ROUTES = { "Local model": "local", "Claude (API)": "anthropic", "OpenAI (API)": "openai", "Gemini (API)": "gemini" };
const OPTS = { hotkey: { "Right Ctrl": "ctrl_r", "Right Shift": "shift_r", "Right Alt": "alt_r", "Pause": "pause", "Scroll Lock": "scroll_lock" }, destination: { "Type into the app I was using": "last_app", "Add to my Prompt Lab draft": "workspace" }, style: { "Tighten & format": "tighten", "Keep wording": "clean" }, target: { "Auto (detect from draft)": "auto", "Coding agent": "claude_code", "General AI": "general" }, judge: DEMO_ROUTES, judge_model: { "Haiku 5.5 · budget": "claude-haiku-5-5", "Sonnet 5.5 · deeper critique": "claude-sonnet-5-5", "Opus 5.5 · detailed review": "claude-opus-5-5" }, engine: DEMO_ROUTES };

// ---------- bridge (pywebview) with a browser-only mock for design work ----------
const mock = {
  _s: { state: "idle", busy: "", lab_ver: 1, lab_busy: "", history_len: 3, llm_ok: true, last_error: "", locked: false, event: "" },
  _lab: { revs: [{ i: 0, kind: "typed", score: 6, stale: false, by: null }, { i: 1, kind: "improve", score: 8, stale: false, by: "claude-opus-5" }], cur: 1,
    text: "Goal: Add a dark mode toggle to the settings page of my Expo app.\n\n<context>\nThe settings page is app/settings.tsx. No new packages.\n</context>\n\n<task>\nImplement the toggle; persist the choice; default to the system setting.\n</task>\n\n<done>\nFlip it, reload, the choice survives.\n</done>",
    by: "claude-opus-5", stale: false, prev_score: 6, fav: null, busy: "", error: "", session_cost: 0.066,
    score: { score: 8, criteria: { clarity: 5, context: 4, task: 5, constraints: 4, success_criteria: 3, format: 5 }, gaps: ["No verification command.", "Persistence API unnamed while new packages are forbidden.", "Theming setup unknown."], improved: "Goal: …improved…", by: "claude-opus-5", cost: 0.033, note: "" } },
  get_state: async () => ({ ...mock._s, recording_ready: mock._s.state === "recording", lab_busy: mock._lab.busy, levels: mock._s.state === "recording" ? Array.from({ length: 20 }, () => Math.random() * 0.1) : [] }),
  get_options: async () => {
    const models={"Parakeet Unified English 0.6B (default)":"parakeet-unified-en-0.6b","Whisper Small":"small","Whisper Medium":"medium","Whisper Large v3":"large-v3","Whisper Large v3 Turbo":"large-v3-turbo"};
    const selected=mock._settings?.speech_model||"parakeet-unified-en-0.6b";
    return {...OPTS, prompt_providers:{
        anthropic:{name:"Claude",configured:true,key_env:"ANTHROPIC_API_KEY"},
        openai:{name:"OpenAI",configured:true,model:"Sample OpenAI model",key_env:"OPENAI_API_KEY"},
        gemini:{name:"Gemini",configured:false,model:"Sample Gemini model",key_env:"GEMINI_API_KEY or GOOGLE_API_KEY"}
      }, speech_models:models,speech_model:selected,speech_model_label:Object.entries(models).find(([,value])=>value===selected)?.[0]||selected,speech_model_note:"Simulated model choices. No installed cache is checked in this demo."};
  },
  get_history: async () => [{ ts: "2026-09-12T10:12:00", kind: "dictate", text: "Add a dark mode toggle to the settings page and remember the choice between launches.", raw: "um add a dark mode toggle", secs: 6.2, latency: 0.9, cost: 0 }, { ts: "2026-09-11T18:02:00", kind: "dictate", text: "Ship it before Friday and let QA know.", raw: "ship it uh before friday", secs: 3.1, latency: 0.7, cost: 0 }],
  get_stats: async () => ({ today: 7, week: 41, words_week: 3120, minutes_week: 22.4, wpm: 139, latency: 0.8, spend_month: 0.42,
    trends: {today:[3,4,5,2,6,5,7], words_week:[2410,2830,2900,3120],
      wpm:[121,127,130,139], latency:[1.2,1.1,1,0.8]} }),
  get_lab: async () => mock._lab,
  _op: (busy, ms, then) => { mock._lab.busy = busy; mock._s.lab_ver++; setTimeout(() => { mock._lab.busy = ""; then && then(); mock._s.lab_ver++; }, ms); return true; },
  score: async () => mock._op("scoring", 800),
  select_rev: async i => { mock._lab.cur = i; mock._s.lab_ver++; return true; },
  copy_text: async () => true, paste_draft: async () => true, clear_history: async () => { mock._s.history_len = 0; return true; },
  diagnostics: async () => ({ speech: "faster-whisper small on CUDA", "clean-up": "qwen2.5:7b via Ollama at 127.0.0.1", "prompt lab": "engineer claude-opus-5, judge claude-opus-5, ANTHROPIC_API_KEY found", files: "…" }),
  get_vocab_suggestions: async () => ["Expo", "Suno", "pywebview"], add_vocab: async () => true, dismiss_vocab: async () => true, lab_kept: async () => true,
  get_learned: async () => ({ lines: ["Kept prompts run about 140 words and use XML-style section tags (7 kept).", "Weakest rubric criterion so far: success criteria (3.2/5)."], snippet: "…", kept: 7, enabled: true }), reset_learned: async () => true,
};
// Synthetic browser demo implements the same acknowledgement contract without side effects.
Object.assign(mock._lab,{draft_id:"demo-1",workspace_id:"demo-1",composer:"",pending:null,saved:true,save_error:""});
mock._archives=[];mock._favorites=[{name:"Demo review",text:"Review this synthetic example",uses:1}];mock._undo={};mock._settings={hotkey:"ctrl_r",command_hotkey:"",destination:"last_app",floating_card:true,sounds:true,restore_clipboard:true,quiet_mode:false,cleanup:true,style:"tighten",auto_app_tone:true,vocab:"ExampleName",target:"auto",judge:"local",engine:"local",local_model:"qwen2.5:7b",engine_local_model:"",judge_local_model:"",cloud_enabled:false,learn:true};
mock._settings.judge_model="claude-haiku-5-5";
mock._settings.live_preview=true;
mock._snippets=[{trigger:"my email",expansion:"alex@example.com"},{trigger:"booking link",expansion:"https://cal.example.com/alex"}];
mock._app_tones=[{app:"Slack",tone:"casual"},{app:"Outlook",tone:"polished"}];
mock._meeting={active:false,transcribing:false,startedAt:0,loopback_active:true,loopback_warning:"",status:"",notes:[{id:1,ts:"2026-11-24T14:30:00",title:"Weekly product sync",duration_secs:742,loopback_used:true,summary:"Aligned on shipping voice snippets, command mode, and local meeting notes this week.",action_items:["Finalize UI integration and run dashboard checks","Verify WASAPI loopback fallback on headphones"],transcript:"Let's review the release checklist for this week. Voice snippets and self-corrections are ready for integration, and local meeting notes will summarize completely offline."}]};
mock.get_settings=async()=>({...mock._settings,snippets:mock._snippets.map(s=>({...s})),app_tone_rules:mock._app_tones.map(r=>({...r}))});
mock.set_setting=async(k,v)=>{
  if(k==="command_hotkey" && v && v===mock._settings.hotkey)return false;
  mock._settings[k]=v;
  return true;
};
mock.get_snippets=async()=>mock._snippets.map(s=>({...s}));
mock.add_snippet=async(trigger,expansion)=>{
  const t=String(trigger||"").trim(), e=String(expansion||"");
  if(!t)return {ok:false,message:"Snippet trigger cannot be empty."};
  if(!e.trim())return {ok:false,message:"Snippet expansion cannot be empty."};
  const i=mock._snippets.findIndex(s=>s.trigger.toLowerCase()===t.toLowerCase());
  if(i>=0)mock._snippets[i]={trigger:t,expansion:e}; else mock._snippets.push({trigger:t,expansion:e});
  return {ok:true,snippets:mock._snippets.map(s=>({...s}))};
};
mock.delete_snippet=async trigger=>{
  const t=String(trigger||"").trim().toLowerCase();
  mock._snippets=mock._snippets.filter(s=>s.trigger.toLowerCase()!==t);
  return {ok:true,snippets:mock._snippets.map(s=>({...s}))};
};
mock.get_app_tones=async()=>({
  auto_enabled:!!mock._settings.auto_app_tone,
  presets:[["casual","Casual"],["polished","Polished"],["technical","Technical"],["default","Default"]],
  rules:mock._app_tones.map(r=>({...r})),
});
mock.add_app_tone=async(app,tone)=>{
  const a=String(app||"").trim(), t=String(tone||"default").toLowerCase();
  if(!a)return {ok:false,message:"App name cannot be empty."};
  const i=mock._app_tones.findIndex(r=>r.app.toLowerCase()===a.toLowerCase());
  if(i>=0)mock._app_tones[i]={app:a,tone:t}; else mock._app_tones.push({app:a,tone:t});
  return {ok:true,rules:mock._app_tones.map(r=>({...r}))};
};
mock.delete_app_tone=async app=>{
  const a=String(app||"").trim().toLowerCase();
  mock._app_tones=mock._app_tones.filter(r=>r.app.toLowerCase()!==a);
  return {ok:true,rules:mock._app_tones.map(r=>({...r}))};
};
mock.get_meetings=async()=>({
  active:mock._meeting.active,
  transcribing:mock._meeting.transcribing,
  elapsed_secs:mock._meeting.active ? Math.max(0, Math.floor((Date.now()-mock._meeting.startedAt)/1000)) : 0,
  loopback_active:mock._meeting.loopback_active,
  loopback_warning:mock._meeting.loopback_warning,
  status:mock._meeting.status,
  notes:mock._meeting.notes.map(n=>({...n,action_items:[...(n.action_items||[])]})),
});
mock.start_meeting=async()=>{
  if(mock._meeting.active || mock._meeting.transcribing)return {ok:false,message:"A meeting is already in progress."};
  mock._meeting.active=true; mock._meeting.startedAt=Date.now(); mock._meeting.loopback_active=true; mock._meeting.loopback_warning=""; mock._meeting.status="Recording locally…";
  return {ok:true,loopback_active:true,loopback_warning:""};
};
mock.stop_meeting=async()=>{
  if(!mock._meeting.active)return {ok:false,message:"No active meeting."};
  const secs=Math.max(1, Math.floor((Date.now()-mock._meeting.startedAt)/1000));
  mock._meeting.active=false; mock._meeting.transcribing=true; mock._meeting.status="Transcribing and summarizing locally…";
  setTimeout(()=>{
    mock._meeting.transcribing=false; mock._meeting.status="";
    mock._meeting.notes.unshift({id:Date.now(),ts:new Date().toISOString().slice(0,19),title:"Demo meeting note",duration_secs:secs,loopback_used:mock._meeting.loopback_active,summary:"Simulated local meeting summary generated in demo mode.",action_items:["Review demo notes and test copy buttons"],transcript:"Simulated meeting transcript captured locally in demo mode."});
  }, 900);
  return {ok:true};
};
mock.cancel_meeting=async()=>{mock._meeting.active=false; mock._meeting.transcribing=false; mock._meeting.status=""; return {ok:true};};
mock.delete_meeting=async id=>{mock._meeting.notes=mock._meeting.notes.filter(n=>Number(n.id)!==Number(id)); return {ok:true};};
let mockHotkeySequence = 0, mockHotkeyToken = null;
mock.begin_hotkey_capture = async () => ({
  ok:true, token:mockHotkeyToken = "demo-" + ++mockHotkeySequence, expires_in:30,
});
mock.check_hotkey_capture = async (token, value) => {
  if (token !== mockHotkeyToken) return {ok:false};
  const blocked = reservedHotkeyMessage(value);
  if (blocked) return {ok:true, level:"blocked", message:blocked};
  if (value === mock._settings.hotkey) return {ok:true, level:"clear"};
  const command = /^ctrl\+[1-5]$/.test(value) ? ["Dictate", "Prompt Lab", "Meetings", "History", "Settings"][+value.at(-1) - 1]
    : {"ctrl+k":"the command menu", "ctrl+s":"Save favorite", "ctrl+enter":"Paste draft"}[value];
  if (command) return {ok:true, level:"warning", message:"Listen uses this shortcut for " + command + "."};
  if (/^ctrl\+[acsvxz]$/.test(value) || /^f\d+$/.test(value)) {
    return {ok:true, level:"warning", message:"Other apps may already use this shortcut."};
  }
  return {ok:true, level:"clear"};
};
mock.finish_hotkey_capture = async (token, value, acceptWarning = false) => {
  if (token !== mockHotkeyToken) return {ok:false};
  if (value) {
    const decision = await mock.check_hotkey_capture(token, value);
    if (!decision.ok || decision.level === "blocked" || (decision.level === "warning" && !acceptWarning)) {
      return {ok:false, message:decision.message};
    }
    await mock.set_setting("hotkey", value);
  }
  mockHotkeyToken = null;
  return {ok:true, hotkey:value};
};
mock.set_text=async(text,id)=>{
  const current=mock._lab;
  if(id!==current.draft_id)return false;
  if(text!==current.text){
    current.text=text;
    if(current.score)current.stale=true;
    if(current.pending)current.pending.stale=true;
    if(current.cur<0 && text.trim()){current.revs.push({i:0,kind:"typed",score:null});current.cur=0;}
    mock._s.lab_ver++;
  }
  return true;
};
mock.set_composer=async(text,id)=>{if(id!==mock._lab.draft_id)return false;mock._lab.composer=text;return true;};
mock.get_local_models=async()=>({models:["qwen2.5:7b","qwen3.5:2b","qwen3.5:4b","qwen3.5:9b"],selected:mock._settings.local_model,available:true,error:""});
mock.get_model_catalog=async()=>({
  speech:[
    {id:"parakeet-unified-en-0.6b",name:"Parakeet Unified English 0.6B",tag:"Built-in · Fastest English",size_mb:640,ram_note:"~0.7 GB RAM · 0 GB VRAM (CPU)",note:"English only. Fast and leaves your GPU completely free.",installed:true,active:mock._settings.speech_model!=="whisper:ggml-large-v3-turbo-q5_0.bin",downloadable:false},
    {id:"whisper:ggml-large-v3-turbo-q5_0.bin",name:"Whisper Large v3 Turbo (Q5_0)",tag:"Best accuracy · Multilingual",size_mb:574,ram_note:"~0.8 GB RAM · 0 GB VRAM (CPU)",note:"Near Large-v3 accuracy with vocabulary hints and multilingual support.",installed:false,active:false,downloadable:true},
  ],
  cleanup:[
    {id:"qwen3.5:4b",name:"Qwen 3.5 4B",tier:"Balanced · ~3.5 GB VRAM",size_gb:"3.4 GB",vram_note:"~3.5 GB VRAM",benchmark:null,note:"Balanced everyday formatting while leaving plenty of GPU headroom.",installed:true,active:mock._settings.local_model==="qwen3.5:4b",downloadable:true},
    {id:"qwen2.5:7b",name:"Qwen 2.5 7B",tier:"Medium · ~5.0 GB VRAM",size_gb:"4.7 GB",vram_note:"~5.0 GB VRAM",benchmark:null,note:"Classic steady formatting baseline.",installed:true,active:mock._settings.local_model==="qwen2.5:7b",downloadable:true},
  ],
  ollama_available:true,
  install:{run_id:0,active:false,done:false,error:""},
  benchmark:{run_id:0,active:false,done:false,error:""},
  benchmarks:{},
  check_note:"Demo model list",
});
mock.check_model_updates=mock.get_model_catalog;
mock.install_model=async()=>({ok:false,message:"Downloads are disabled in demo mode."});
mock.get_install_status=async()=>({run_id:0,active:false,done:false,error:""});
mock.cancel_install=async()=>false;
mock.start_benchmark=async()=>({ok:false,message:"Benchmarks are disabled in demo mode."});
mock.get_benchmark_status=async()=>({run_id:0,active:false,done:false,error:""});
mock.cancel_benchmark=async()=>false;
mock.list_drafts=async()=>mock._archives.map(r=>({id:r.draft_id,title:r.text.slice(0,50)||"Empty draft"}));
mock.new_draft=async()=>{mock._archives.push(structuredClone(mock._lab));Object.assign(mock._lab,{draft_id:"demo-"+Date.now(),workspace_id:"demo-"+Date.now(),text:"",composer:"",pending:null,revs:[],cur:-1,score:null});mock._s.lab_ver++;return true;};
mock.restore_draft=async id=>{const i=mock._archives.findIndex(r=>r.draft_id===id);if(i<0)return false;const next=mock._archives.splice(i,1)[0];mock._archives.push(structuredClone(mock._lab));mock._lab=next;mock._s.lab_ver++;return true;};
const demoAssessment=mock._lab.score;
function putDemoCandidate(kind,text){
  const version={text,by:"Synthetic demo model",cost:0,score:null,based_on:null};
  mock._lab.pending={...version,id:"candidate-"+Date.now(),kind,source_text:mock._lab.text,stale:false,versions:[version]};
}
const demoCandidate=async(kind,text)=>{
  if(mock._lab.pending || mock._lab.busy)return false;
  const {draft_id, text:original}=mock._lab;
  return mock._op("generating",600,()=>{
    if(!mock._lab.pending && draft_id===mock._lab.draft_id && original===mock._lab.text)
      putDemoCandidate(kind,text||original+"\n\nVerify the result with a focused check.");
  });
};
mock.improve=()=>demoCandidate("improve");mock.rebuild=()=>demoCandidate("rebuild");mock.submit=text=>demoCandidate("refine",mock._lab.text+"\n\nInstruction: "+text);
mock.feedback=async(id,text)=>{
  if(mock._lab.busy || id!==mock._lab.draft_id || text!==mock._lab.text || !text.trim())return false;
  const request="feedback-"+Date.now(), improved=text+"\n\nVerify the result with a focused check.";
  mock._op("scoring",600,()=>{
    if(id!==mock._lab.draft_id || text!==mock._lab.text)return;
    mock._lab.score={...demoAssessment,improved,feedback_id:request,by:"Synthetic demo model",cost:0};
    mock._lab.stale=false;
    if(!mock._lab.pending)putDemoCandidate("improve",improved);
  });
  return request;
};
mock.draft_pass=async(id,text,refine)=>{
  if(mock._lab.busy || id!==mock._lab.draft_id || text!==mock._lab.text || !text.trim())return false;
  const request="draft-pass-"+Date.now();
  mock._op(refine ? "refining" : "engineering",600,()=>{
    if(id!==mock._lab.draft_id || text!==mock._lab.text)return;
    const held=refine && mock._lab.score?.score>=9;
    if(!held){
      const grade=refine ? Math.min(9,(mock._lab.score?.score||7)+1) : 7;
      mock._lab.text=text+(refine ? "\n\nInclude a clear success check." : "\n\nUse the request above as the brief. Follow its constraints.");
      mock._lab.draft_id="demo-"+request;
      mock._lab.score={...demoAssessment,score:grade,improved:"",by:"Synthetic demo model",cost:0};
      mock._lab.stale=false;
      mock._lab.cur=mock._lab.revs.length;
      mock._lab.revs.push({i:mock._lab.cur,score:grade,kind:refine ? "refine" : "engineer",by:"Synthetic demo model",stale:false});
    }
    mock._lab.draft_pass={id:request,source_id:id,draft_id:mock._lab.draft_id,message:held ? "No meaningful improvement this pass. Your draft and score are unchanged." : "Draft updated."};
  });
  return request;
};
mock.review_candidate=async(id,index)=>{
  const pending=mock._lab.pending, version=pending?.versions[index];
  if(mock._lab.busy || pending?.id!==id || pending.stale || !version)return false;
  return mock._op("reviewing",600,()=>{
    if(mock._lab.pending!==pending || pending.stale)return;
    const check="Include the result of the check.";
    const improved=version.text.includes(check) ? version.text : version.text+"\n"+check;
    version.score={...demoAssessment,score:9,gaps:improved===version.text?[]:["Spell out what to report after verification."],improved,by:version.by,cost:0};
    if(improved!==version.text){
      pending.versions.push({text:improved,by:version.by,cost:0,score:null,based_on:index,basis_score:version.score});
      pending.text=improved;
    }
  });
};
mock.apply_candidate=async (id,index)=>{
  const p=mock._lab.pending,version=index===undefined?p?.versions.at(-1):p?.versions[index];
  if(!p||p.id!==id||p.stale||p.source_text!==mock._lab.text||!version||mock._lab.busy==="reviewing")return false;
  mock._lab.text=version.text;
  mock._lab.revs.push({i:mock._lab.revs.length,kind:p.kind,by:version.by,score:version.score?.score??null});
  Object.assign(mock._lab,{cur:mock._lab.revs.length-1,by:version.by,score:version.score,stale:false,pending:null});
  mock._s.lab_ver++;
  return true;
};
mock.discard_candidate=async id=>{if(mock._lab.pending?.id!==id)return false;mock._lab.pending=null;mock._s.lab_ver++;return true;};
mock.get_favorites=async()=>mock._favorites;mock.save_favorite_checked=async(name,text,replace=false)=>{const old=mock._favorites.find(f=>f.name===name);if(old&&!replace)return {status:"conflict"};if(old)old.text=text;else mock._favorites.push({name,text,uses:0});return {status:"saved"};};
mock.remove_favorite=async name=>{const i=mock._favorites.findIndex(f=>f.name===name);if(i<0)return null;const token="undo-"+Date.now();mock._undo[token]=mock._favorites.splice(i,1)[0];return token;};
mock.undo_favorite_removal=async token=>{const f=mock._undo[token];if(!f||mock._favorites.some(x=>x.name===f.name))return false;mock._favorites.push(f);delete mock._undo[token];return true;};
mock.load_favorite=async name=>{const f=mock._favorites.find(x=>x.name===name);if(!f)return false;await mock.new_draft();mock._lab.text=f.text;mock._s.lab_ver++;return true;};
mock.get_retained=async()=>[];mock.get_microphones=async()=>({"Demo microphone (simulated)":null});
mock.delete_draft=async(id,confirmation)=>{if(confirmation!=="DELETE")return false;const i=mock._archives.findIndex(x=>String(x.draft_id)===String(id));if(i<0)return false;mock._archives.splice(i,1);return true;};
mock.finish_recording=async()=>{mock._s.state="idle";return true;};mock.cancel_recording=mock.finish_recording;mock.retry_startup=mock.finish_recording;
mock.clear_private_data=async(scope,confirmation)=>{if(confirmation!=="DELETE")return {ok:false,message:"Canceled"};if(scope==="workspace"){mock._archives=[];await mock.new_draft();mock._archives=[];}if(scope==="history")mock.get_history=async()=>[];return {ok:true,message:"Demo "+scope+" cleared (synthetic data only)"};};
const demo = new URLSearchParams(location.search).get("demo") === "1";
let bridgeReady = !!window.pywebview?.api, bridgeLost = false, booted = false;
const api = () => { if (demo) return mock; if (!bridgeReady || !window.pywebview?.api) throw new Error("Desktop connection unavailable"); return window.pywebview.api; };
function connection(message, retry) {
  const status = $("#bridge-status"), button = $("#btn-check-connection");
  const wasWaiting = document.body.classList.contains("connection-wait");
  $("#bridge-notice").classList.toggle("hidden", !message);
  status.classList.toggle("hidden", !message);
  if (status.textContent !== message) status.textContent = message;
  button.classList.toggle("hidden", !retry);
  button.onclick = retry || null;
  document.body.classList.toggle("connection-wait", !!message);
  if (message && !document.body.classList.contains("recording")) {
    setDisplayedProperty($("#dictate-heading"), "textContent", "Checking Listen…");
    setDisplayedProperty($("#dictate-instruction"), "textContent", "See the connection status above.");
  } else if (!message && wasWaiting && S.state) setStatus(S);
}
function recovery(message) {
  const el = $("#recovery-status");
  if (el.textContent !== message) {
    el.textContent = message;
    el.dataset.saved = String(message === "Saved locally");
  }
}
const htmlCache = new WeakMap();
function stableHTML(el, html) { if (htmlCache.get(el) !== html) { el.innerHTML = html; htmlCache.set(el, html); } }


// ---------- state ----------
let S = {}, settings = {}, options = {}, lab = { revs: [], text: "" }, favs = [];
let lastLabVer = -1, lastHistLen = -1, page = "dictate", editing = false;
let feedbackRequest;
const feedbackIssues = new Map();
let labClearVersion = 0;

// ---------- live preview ----------
let previewUiEpoch = 0, previewLastVersion = -1, previewActionPending = 0, previewSettingVersion = 0;
function clearPreviewDom() {
  $("#live-preview").classList.add("hidden");
  for(const id of ["#live-preview-text","#live-preview-phase"]) {
    const element=$(id);if(element.textContent)element.textContent="";
  }
}
function invalidatePreview(version) {
  previewUiEpoch++;
  if(Number.isInteger(version))previewLastVersion=Math.max(previewLastVersion,version);
  clearPreviewDom();
}
function renderPreview(st, epoch) {
  if(epoch!==previewUiEpoch)return;
  if(settings.live_preview===false||previewActionPending) {clearPreviewDom();return;}
  const preview=st.preview||{};
  if(!Number.isInteger(preview.version)) {clearPreviewDom();return;}
  if(preview.version<previewLastVersion)return;
  previewLastVersion=preview.version;
  if(preview.phase==="final"&&Number.isFinite(preview.dismiss_at)&&Number.isFinite(st.preview_now)&&st.preview_now>=preview.dismiss_at){clearPreviewDom();return;}
  if(!preview.active&&!["finishing","final"].includes(preview.phase)) {clearPreviewDom();return;}
  const text=typeof preview.text==="string"?preview.text:"";
  const phase=preview.phase==="finishing" ? "Finishing the full recording…"
    : preview.phase==="final" ? preview.final_outcome==="review" ? "Needs review in Listen. Nothing sent." : preview.final_outcome==="failed" ? "Final processing failed. Check Listen." : "Final recognized transcript."
    : preview.state==="paused" ? preview.reason==="slow" ? "Preview paused to keep final dictation responsive." : "Preview is unavailable. Recording controls remain available."
    : preview.state==="updating" ? "Updating preview…" : "Listening…";
  $("#live-preview").classList.remove("hidden");
  const heading=preview.phase==="final"?"Final recognized transcript":"Recording transcript · provisional";
  const note=preview.phase==="final"?"The actual result and delivery status are shown in Listen.":"Provisional. Final text is generated when you finish.";
  if($("#live-preview-title").textContent!==heading)$("#live-preview-title").textContent=heading;
  if($("#live-preview-note").textContent!==note)$("#live-preview-note").textContent=note;
  if($("#live-preview-text").textContent!==text)$("#live-preview-text").textContent=text;
  if($("#live-preview-phase").textContent!==phase)$("#live-preview-phase").textContent=phase;
}

// ---------- toasts ----------
const TOAST_MS = { info: 2400, err: 8000 };
function toast(msg, err = false) {
  const t = document.createElement("div"); t.className = "toast" + (err ? " err" : ""); t.setAttribute("role", err ? "alert" : "status");
  const m = document.createElement("span"); m.textContent = msg; t.appendChild(m);
  let timer;
  const close = () => { clearTimeout(timer); t.classList.add("out"); setTimeout(() => t.remove(), 300); };
  const arm = () => { clearTimeout(timer); timer = setTimeout(close, err ? TOAST_MS.err : TOAST_MS.info); };
  if (err) { const x = document.createElement("button"); x.className = "icon-btn"; x.setAttribute("aria-label", "Dismiss"); x.innerHTML = '<svg class="ico" aria-hidden="true"><use href="#i-x"/></svg>'; x.onclick = close; t.appendChild(x); }
  t.onmouseenter = t.onfocusin = () => clearTimeout(timer); t.onmouseleave = t.onfocusout = arm;
  $("#toasts").appendChild(t); arm();
}
// Replaces window.confirm(): the same labelled <dialog> pattern as favorite naming. Resolves true only on confirm.
function askConfirm(message, opts = {}) {
  const dialog = $("#confirm-dialog"), opener = document.activeElement;
  if (dialog.open) return Promise.resolve(false);
  const { title = "Please confirm", confirmLabel = "Continue", destructive = false } = opts;
  $("#confirm-title").textContent = title;
  $("#confirm-message").textContent = message;
  const okButton = $("#confirm-ok");
  if (okButton) {
    okButton.textContent = confirmLabel;
    okButton.classList.toggle("danger", !!destructive);
    okButton.classList.toggle("primary", !destructive);
  }
  dialog.returnValue = "";
  return new Promise(resolve => { dialog.addEventListener("close", () => { opener?.focus(); resolve(dialog.returnValue === "ok"); }, { once: true }); dialog.showModal(); });
}

// ---------- pages ----------
function show(p, focus = true) {
  for (const panel of $$(".lab-popover:popover-open")) panel.hidePopover();
  const entering = page !== p;
  if (entering) {
    $(".main").scrollTop = 0;
    if (feedbackRequest) feedbackRequest.leftLab = true;
  }
  page = p;
  $$(".page").forEach(el => el.classList.toggle("active", el.dataset.page === p));
  $$(".rail-btn").forEach(b => { const on = b.dataset.page === p; b.classList.toggle("active", on); if (on) b.setAttribute("aria-current", "page"); else b.removeAttribute("aria-current"); });
  closeMore(false);
  if (p === "meetings") loadMeetings(true);
  else if (!shouldPollMeetings()) stopMeetingsPoll();
  if (p === "history") loadHistory(entering);
  if (p === "settings") renderSettings();
  if (focus) (p === "lab" ? $("#draft") : $(`.page[data-page="${p}"] h1`))?.focus();
}
$$(".rail-btn").forEach(b => b.onclick = () => show(b.dataset.page));
$$("[data-goto]").forEach(b => b.onclick = () => show(b.dataset.goto));
const hotkeyDialog = $("#hotkey-dialog"), hotkeyCapture = $("#hotkey-capture");
const hotkeyCancel = $("#hotkey-cancel"), hotkeyAgain = $("#hotkey-again"), hotkeyUse = $("#hotkey-use");
const hotkeyController = $("#hotkey-controller");
const hotkeySingles = {ControlRight:"ctrl_r", ShiftRight:"shift_r", AltRight:"alt_r", Pause:"pause", ScrollLock:"scroll_lock"};
const hotkeyNamedTriggers = {ArrowLeft:"arrow_left", ArrowRight:"arrow_right", ArrowUp:"arrow_up", ArrowDown:"arrow_down",
  Home:"home", End:"end", Insert:"insert", Delete:"delete", PageUp:"page_up", PageDown:"page_down",
  Backspace:"backspace", Enter:"enter", NumpadEnter:"enter"};
const hotkeyHeld = new Set(), hotkeyOpenerKeys = new Set();
let hotkeyLease = null, hotkeyOpening = false, hotkeyMode = "listening";
let hotkeyTrigger = null, hotkeyChoice = "", hotkeyBlock = "", hotkeyMultiple = false;
let controllerHotkeyAttempt = null;

document.addEventListener("keydown", event => {
  if ((event.key === "Enter" || event.key === " ") && !event.isComposing && event.keyCode !== 229
      && event.target.closest?.("#btn-hotkey-card, #settings-hotkey, #palette-input")) {
    hotkeyOpenerKeys.add(event.code || event.key);
  }
}, true);
document.addEventListener("keyup", event => hotkeyOpenerKeys.delete(event.code || event.key), true);
window.addEventListener("blur", () => hotkeyOpenerKeys.clear());

function waitForHotkeyOpener() {
  if (!hotkeyOpenerKeys.size) return true;
  return new Promise(resolve => {
    const finish = released => {
      document.removeEventListener("keyup", release, true);
      window.removeEventListener("blur", cancel);
      resolve(released);
    };
    const release = () => { if (!hotkeyOpenerKeys.size) finish(true); };
    const cancel = () => finish(false);
    document.addEventListener("keyup", release, true);
    window.addEventListener("blur", cancel);
  });
}

function reservedHotkeyMessage(value) {
  const keys = value.split("+");
  if (keys.includes("win")) return "Windows reserves shortcuts that use the Windows key.";
  if (keys.at(-1) === "f12") return "Windows reserves F12 for debugging.";
  if (value === "alt+f4") return "Windows uses Alt + F4 to close the current window.";
  return "";
}

function hotkeyTriggerValue(key, code) {
  if (key === " ") return "space";
  if (/^[a-z0-9]$/i.test(key)) return key.toLowerCase();
  if (/^F(?:[1-9]|1\d|2[0-4])$/.test(code)) return code.toLowerCase();
  if (/^Digit[0-9]$/.test(code)) return code.at(-1);
  return hotkeyNamedTriggers[key] || hotkeyNamedTriggers[code] || "";
}

function resetHotkeyKeys() {
  hotkeyHeld.clear();
  hotkeyTrigger = null;
  hotkeyChoice = hotkeyBlock = "";
  hotkeyMultiple = false;
  $("#hotkey-key").textContent = "Listening for keys…";
}

function resetHotkeyPicker() {
  hotkeyMode = "listening";
  controllerHotkeyAttempt = null;
  resetHotkeyKeys();
  hotkeyCapture.disabled = hotkeyCancel.disabled = hotkeyAgain.disabled = hotkeyUse.disabled = false;
  let controllerAvailable = false;
  try { controllerAvailable = typeof api().begin_controller_hotkey_capture === "function"; } catch {}
  hotkeyController.classList.toggle("hidden", !controllerAvailable);
  $("#hotkey-options").textContent = "";
  $("#hotkey-windows-note").classList.add("hidden");
  $("#hotkey-conflict").classList.add("hidden");
  hotkeyAgain.classList.add("hidden");
  hotkeyUse.classList.add("hidden");
}

async function openHotkeyPicker(opener) {
  if (hotkeyOpening || hotkeyLease) return;
  hotkeyOpening = true;
  const sourcePage = page;
  try {
    if (!await waitForHotkeyOpener() || sourcePage !== page || !opener?.isConnected || !document.hasFocus()) return;
    const started = performance.now();
    const capture = await api().begin_hotkey_capture();
    if (!capture?.ok || !capture.token) {
      toast("Finish the current recording or release the hotkey before trying again.", true);
      return;
    }
    const expiresAt = started + capture.expires_in * 1000 - 1000;
    const remaining = expiresAt - performance.now();
    hotkeyLease = {token:capture.token, expiresAt, opener};
    if (sourcePage !== page || !opener?.isConnected || !document.hasFocus() || remaining <= 0) {
      await api().finish_hotkey_capture(capture.token);
      hotkeyLease = null;
      if (remaining <= 0) toast("Hotkey setup timed out. Click Hotkey to try again.");
      return;
    }
    resetHotkeyPicker();
    hotkeyDialog.showModal();
    hotkeyCapture.focus();
    const lease = hotkeyLease;
    lease.timer = setTimeout(() => {
      if (hotkeyLease === lease && hotkeyDialog.open && hotkeyMode !== "saving") {
        hotkeyDialog.close();
        toast("Hotkey setup timed out. Click Hotkey to try again.");
      }
    }, remaining);
  } catch {
    const token = hotkeyLease?.token;
    hotkeyLease = null;
    if (token) api().finish_hotkey_capture(token).catch(() => {});
    toast("Could not start hotkey setup. Try again.", true);
  } finally { hotkeyOpening = false; }
}

function showHotkeyCheck(level, message = "") {
  hotkeyMode = "checked";
  $("#hotkey-options").textContent = "";
  $("#hotkey-conflict-title").textContent = level === "blocked" ? "Choose another shortcut" : "Possible shortcut conflict";
  $("#hotkey-conflict-message").textContent = message;
  $("#hotkey-conflict").classList.toggle("hidden", level === "clear");
  $("#hotkey-windows-note").classList.toggle("hidden",
    demo || level === "blocked" || hotkeyChoice.startsWith("controller_") || Object.values(hotkeySingles).includes(hotkeyChoice));
  hotkeyAgain.classList.remove("hidden");
  hotkeyUse.classList.toggle("hidden", level === "blocked");
  hotkeyUse.textContent = level === "warning" ? "Use anyway" : "Use hotkey";
  hotkeyUse.dataset.warning = String(level === "warning");
  (level === "clear" ? hotkeyUse : hotkeyAgain).focus();
}

async function checkHotkeyChoice() {
  const lease = hotkeyLease;
  hotkeyMode = "checking";
  hotkeyCapture.disabled = true;
  hotkeyController.classList.add("hidden");
  hotkeyCancel.focus();
  if (hotkeyBlock) {
    showHotkeyCheck("blocked", hotkeyBlock);
    return;
  }
  $("#hotkey-options").textContent = "Checking the shortcut…";
  try {
    const result = await api().check_hotkey_capture(lease.token, hotkeyChoice);
    if (hotkeyLease !== lease || !hotkeyDialog.open) return;
    if (result?.ok && ["clear", "warning", "blocked"].includes(result.level)) {
      showHotkeyCheck(result.level, result.message);
      return;
    }
    showHotkeyCheck("blocked", result?.message || "Could not check this shortcut. Try again.");
  } catch {
    if (hotkeyLease === lease && hotkeyDialog.open) {
      showHotkeyCheck("blocked", "Could not check this shortcut. Try again.");
    }
  }
}

hotkeyController.onclick = async () => {
  const lease = hotkeyLease;
  if (!lease || !hotkeyDialog.open || hotkeyMode !== "listening") return;
  hotkeyMode = "starting-controller";
  hotkeyCapture.disabled = true;
  hotkeyController.classList.add("hidden");
  hotkeyCancel.focus();
  $("#hotkey-key").textContent = "Checking for a controller…";
  let result;
  try { result = await api().begin_controller_hotkey_capture(lease.token); }
  catch { /* Show the available keyboard picker again after a connection failure. */ }
  if (hotkeyLease !== lease || !hotkeyDialog.open) return;
  if (!result?.ok) {
    resetHotkeyPicker();
    $("#hotkey-options").textContent = result?.message || "Could not start controller capture. Try again.";
    hotkeyCapture.focus();
    return;
  }
  controllerHotkeyAttempt = {token:lease.token};
  hotkeyMode = "controller";
  $("#hotkey-key").textContent = "Tap a controller button, then let go.";
};

function receiveControllerHotkey(attempt, capture) {
  if (!attempt || attempt !== controllerHotkeyAttempt || !hotkeyDialog.open || hotkeyMode !== "controller"
      || capture?.token !== attempt.token || typeof capture.binding !== "string"
      || !/^controller_[a-z0-9]+(?:_[a-z0-9]+)*$/.test(capture.binding)) return;
  controllerHotkeyAttempt = null;
  hotkeyChoice = capture.binding;
  renderHotkeyKeycaps($("#hotkey-key"), hotkeyLabel(hotkeyChoice));
  checkHotkeyChoice();
}

function useSavedHotkey(value) {
  settings.hotkey = value;
  renderChips();
  syncSettings();
  setStatus(S);
  hotkeyDialog.close();
  toast("Hotkey saved.");
}

$("#btn-hotkey-card").onclick = event => openHotkeyPicker(event.currentTarget);
hotkeyCapture.addEventListener("keydown", event => {
  if (event.key === "Tab" || event.key === "Escape") return;
  event.preventDefault();
  event.stopPropagation();
  if (hotkeyMode !== "listening" || event.repeat || event.isComposing || event.keyCode === 229) return;
  $("#hotkey-options").textContent = "";
  const code = event.code || event.key;
  hotkeyHeld.add(code);
  if (event.key === "AltGraph" || event.getModifierState("AltGraph")) {
    hotkeyBlock = "AltGr enters characters on this keyboard. Choose another shortcut.";
  }
  const mods = [["ctrl", event.ctrlKey], ["alt", event.altKey], ["shift", event.shiftKey],
    ["win", event.metaKey || code.startsWith("Meta")]].filter(([, active]) => active).map(([name]) => name);
  const modifier = /^(?:Control|Alt|Shift|Meta)(?:Left|Right)$/.test(code);
  if (!modifier) {
    if (hotkeyTrigger && hotkeyTrigger.code !== code) hotkeyMultiple = true;
    hotkeyTrigger = {code, key:hotkeyTriggerValue(event.key, code)};
  }
  if (!hotkeyTrigger || hotkeyHeld.has(hotkeyTrigger.code)) {
    const key = hotkeyTrigger?.key;
    const single = hotkeySingles[hotkeyTrigger?.code || code];
    const chord = mods.length && key ? [...mods, key].join("+") : "";
    const canUseSingle = modifier ? mods.length <= 1 : mods.length === 0;
    hotkeyChoice = chord || (canUseSingle && !key ? single : "")
      || (!mods.length && /^f\d+$/.test(key || "") ? key : "");
    const label = hotkeyChoice || mods.join("+");
    const display = $("#hotkey-key");
    if (label) renderHotkeyKeycaps(display, hotkeyLabel(label));
    else display.textContent = "Try a function key or a combination";
  }
  if (mods.includes("win")) hotkeyBlock = reservedHotkeyMessage("win");
  if (hotkeyChoice) hotkeyBlock ||= reservedHotkeyMessage(hotkeyChoice);
});
hotkeyCapture.addEventListener("keyup", event => {
  if (hotkeyMode !== "listening" || !hotkeyLease) return;
  if (!hotkeyHeld.delete(event.code || event.key) && !hotkeyChoice && !hotkeyBlock) return;
  event.preventDefault();
  event.stopPropagation();
  if (hotkeyHeld.size || event.ctrlKey || event.altKey || event.shiftKey || event.metaKey) return;
  if (hotkeyMultiple || (!hotkeyChoice && !hotkeyBlock)) {
    resetHotkeyKeys();
    $("#hotkey-options").textContent = "Try a function key, or hold Ctrl, Alt or Shift and press one other key.";
    return;
  }
  checkHotkeyChoice();
});
hotkeyCapture.addEventListener("blur", () => { if (hotkeyMode === "listening") resetHotkeyKeys(); });
hotkeyAgain.onclick = () => { resetHotkeyPicker(); hotkeyCapture.focus(); };
for (const button of [hotkeyUse, hotkeyController]) {
  button.addEventListener("keydown", event => { if (event.key === "Enter") event.preventDefault(); });
  button.addEventListener("keyup", event => {
    if (event.key !== "Enter") return;
    event.preventDefault();
    button.click();
  });
}
hotkeyUse.onclick = async () => {
  const lease = hotkeyLease;
  if (!lease || hotkeyMode !== "checked" || hotkeyUse.classList.contains("hidden")) return;
  hotkeyMode = "saving";
  hotkeyCancel.disabled = hotkeyAgain.disabled = hotkeyUse.disabled = true;
  $("#hotkey-options").textContent = "Saving…";
  try {
    const result = await api().finish_hotkey_capture(lease.token, hotkeyChoice, hotkeyUse.dataset.warning === "true");
    if (result?.ok && result.hotkey === hotkeyChoice) {
      useSavedHotkey(result.hotkey);
      return;
    }
    showHotkeyCheck("blocked", result?.message || "Could not save this shortcut. Pick again to retry.");
  } catch {
    try {
      if ((await api().get_settings()).hotkey === hotkeyChoice) {
        useSavedHotkey(hotkeyChoice);
        return;
      }
    } catch { /* Settings can be checked again after reconnecting. */ }
    showHotkeyCheck("blocked", "Could not confirm the save. Check your saved shortcut before trying again.");
  } finally {
    hotkeyCancel.disabled = hotkeyAgain.disabled = hotkeyUse.disabled = false;
    if (hotkeyMode !== "saving" && hotkeyDialog.open) {
      if (performance.now() >= lease.expiresAt) {
        hotkeyDialog.close();
        toast("Hotkey setup timed out. Click Hotkey to try again.");
      } else hotkeyAgain.focus();
    }
  }
};
hotkeyCancel.onclick = () => hotkeyDialog.close();
hotkeyDialog.addEventListener("cancel", event => { if (hotkeyMode === "saving") event.preventDefault(); });
window.addEventListener("blur", () => { if (hotkeyDialog.open && hotkeyMode !== "saving") hotkeyDialog.close(); });
hotkeyDialog.addEventListener("close", async () => {
  const lease = hotkeyLease;
  if (!lease) return;
  controllerHotkeyAttempt = null;
  clearTimeout(lease.timer);
  if (lease.opener.isConnected && document.hasFocus()) lease.opener.focus();
  try { await api().finish_hotkey_capture(lease.token); }
  catch { /* The native lease expires even if the bridge disconnects. */ }
  finally { hotkeyLease = null; }
});

$(".main").addEventListener("focusin", event => {
  if (!event.target.matches(".frame-input, .editor")) return;
  const frame = event.target.closest("fieldset.frame"), workspace = $(".main");
  if (!frame) return;
  const viewport = workspace.getBoundingClientRect(), bounds = frame.getBoundingClientRect();
  const top = bounds.top - viewport.top, bottom = bounds.bottom - viewport.top;
  if (bounds.height > workspace.clientHeight - 36 || top < 18) workspace.scrollTop += top - 18;
  else if (bottom > workspace.clientHeight - 18) workspace.scrollTop += bottom - workspace.clientHeight + 18;
});

// ---------- bars ----------
function mkBars(el, n = 20) { el.innerHTML = Array.from({ length: n }, () => "<i></i>").join(""); }
mkBars($("#bars-dictate")); mkBars($("#bars-lab"));
const renderedBarHeights = new WeakMap();
function drawBars(levels) {
  const lv = [...Array(Math.max(0, 20 - levels.length)).fill(0), ...levels.slice(-20)];
  for (const el of [$("#bars-dictate"), $("#bars-lab")])
    $$("i", el).forEach((b, i) => {
      const height = (4 + Math.min(1, lv[i] * 6) * 22) + "px", previous = renderedBarHeights.get(b);
      if (previous?.requested === height && previous.applied === b.style.height) return;
      b.style.height = height;
      renderedBarHeights.set(b, {requested: height, applied: b.style.height});
    });
}

// ---------- chips ----------
let promptModelNames = [], promptInventoryChecked = false;
function promptLocalChoices(key) {
  const selected = settings[key] || "";
  const fallback = settings.local_model ? `Use formatting model (${settings.local_model})` : "Use formatting model";
  const option = (model, label, unavailable = false) => `<option value="${esc(model)}"${selected === model ? " selected" : ""}${unavailable ? " disabled" : ""}>${esc(label)}</option>`;
  const missing = selected && !promptModelNames.includes(selected);
  return option("", fallback) + (missing
    ? option(selected, selected + (promptInventoryChecked ? " (saved; not installed)" : " (saved; list unavailable)"), true) : "")
    + promptModelNames.map(model => option(model, model)).join("");
}
function rememberPromptModels(data) {
  const names = [...new Set((data.models || []).filter(name => typeof name === "string" && name))];
  const checked = data.available === true || (data.available !== false && !data.error);
  if (JSON.stringify(names) === JSON.stringify(promptModelNames) && checked === promptInventoryChecked) return;
  promptModelNames = names;
  promptInventoryChecked = checked;
  renderChips();
}
function promptRouteSummary(stage) {
  const selected = settings[stage];
  if (!selected || selected === "local") {
    const choice = settings[stage + "_local_model"];
    if (choice && promptInventoryChecked && !promptModelNames.includes(choice)) return `${choice} is saved but isn’t installed. Choose another model or start Ollama and refresh.`;
    return `Uses ${choice || settings.local_model || "your formatting model"} on this computer.`;
  }
  const provider = options.prompt_providers?.[selected];
  const name = provider?.name || Object.entries(options[stage] || {}).find(([, value]) => value === selected)?.[0] || "Cloud provider";
  if (!settings.cloud_enabled) return `${name} selected. Allow paid cloud AI before using this step.`;
  if (provider && !provider.configured) return `${name} API key missing. Set ${provider.key_env} on this PC.`;
  return `${name}${provider?.model ? " · " + provider.model : ""}. Paid cloud. Text is sent when you ask for this step.`;
}
function hotkeyLabel(value) {
  if (value?.startsWith("controller_")) {
    const key = value.slice("controller_".length);
    const labels = {a:"A", b:"B", x:"X", y:"Y", dpad_up:"D-pad Up", dpad_down:"D-pad Down",
      dpad_left:"D-pad Left", dpad_right:"D-pad Right", left_bumper:"Left Bumper", right_bumper:"Right Bumper",
      left_stick:"Left Stick", right_stick:"Right Stick", start:"Start", back:"Back"};
    return "Controller " + (labels[key] || key.split("_").map(word => word[0]?.toUpperCase() + word.slice(1)).join(" "));
  }
  const labels = {ctrl_r:"Right Ctrl", shift_r:"Right Shift", alt_r:"Right Alt", pause:"Pause", scroll_lock:"Scroll Lock",
    ctrl:"Ctrl", alt:"Alt", shift:"Shift", win:"Win", space:"Space", arrow_left:"Left Arrow", arrow_right:"Right Arrow",
    arrow_up:"Up Arrow", arrow_down:"Down Arrow", home:"Home", end:"End", insert:"Insert", delete:"Delete",
    page_up:"Page Up", page_down:"Page Down", backspace:"Backspace", enter:"Enter"};
  return Object.entries(options.hotkey || {}).find(([, binding]) => binding === value)?.[0]
    || (value || "ctrl_r").split("+").map(key => labels[key] || key.toUpperCase()).join(" + ");
}
function renderHotkeyKeycaps(element, label) {
  const keys = label.split(" + ");
  const arrows = {"Left Arrow":"←", "Right Arrow":"→", "Up Arrow":"↑", "Down Arrow":"↓"};
  element.classList.toggle("combo", keys.length > 1);
  element.classList.toggle("compact", keys.length >= 4 || keys.some(key => key.length > 12));
  const html = keys.map((key, i) => `<span class="hotkey-part">${i ? "<span>+</span>" : ""}<kbd>${Object.hasOwn(arrows, key)
    ? `<span aria-hidden="true">${arrows[key]}</span><span class="sr-only">${key}</span>` : esc(key)}</kbd></span>`).join("");
  if (element.innerHTML !== html) element.innerHTML = html;
}
function optionEntries(key) {
  const entries = Object.entries(options[key] || {});
  if (key === "engine" || key === "judge") {
    entries.sort(([, a], [, b]) => (a === b ? 0 : a === "local" ? -1 : b === "local" ? 1 : 0));
  }
  return entries;
}
function chip(key, label, icon, customChoices) {
  const choices = customChoices ?? optionEntries(key).map(([n, v]) => `<option value="${esc(v)}"${settings[key] === v ? " selected" : ""}>${esc(n)}</option>`).join("");
  return `<label class="chip"><svg class="ico" aria-hidden="true"><use href="#${icon}"/></svg><span class="k">${label}</span><select data-key="${key}">${choices}</select></label>`;
}
function renderChips() {
  const a = document.activeElement, box = a?.closest?.(".chips")?.id;
  const sel = a?.dataset?.key ? `[data-key="${a.dataset.key}"]` : "";
  const selected = [settings.engine, settings.judge].filter(value => value && value !== "local");
  const cloudSelected = selected.length > 0, cloudActive = cloudSelected && settings.cloud_enabled;
  const missingKey = cloudActive && selected.some(route => options.prompt_providers?.[route]?.configured === false);
  const processing = cloudActive ? (missingKey ? "Cloud API key needed" : "Cloud API processing · paid calls")
    : cloudSelected ? "Paid cloud is off" : "Local processing · stays on this computer";
  stableHTML($("#chips-lab"), `<span class="processing-label">${processing}</span>`
    + chip("target", "Target", "i-clipboard") + chip("engine", "Write", "i-sparkle")
    + (settings.engine === "local" ? chip("engine_local_model", "Writing model", "i-sparkle", promptLocalChoices("engine_local_model")) : "")
    + chip("judge", "Check", "i-chart")
    + (settings.judge === "local" ? chip("judge_local_model", "Checking model", "i-chart", promptLocalChoices("judge_local_model"))
      : settings.judge === "anthropic" ? chip("judge_model", "Claude judge", "i-chart") : "")
    + `<div class="muted">Write · ${esc(promptRouteSummary("engine"))}<br>Check · ${esc(promptRouteSummary("judge"))}</div>`);
  $$(".chip select").forEach(s => s.onchange = () => setSetting(s.dataset.key, s.value));
  const hotkeyName = hotkeyLabel(settings.hotkey);
  const rightCtrl = originalPicture && hotkeyName === "Right Ctrl";
  renderHotkeyKeycaps($("#hero-key"), rightCtrl ? "Ctrl" : hotkeyName);
  if (pictureKeySide) pictureKeySide.hidden = !rightCtrl;
  $("#btn-hotkey-card").setAttribute("aria-label", "Change dictation hotkey, " + hotkeyName);
  $("#composer-key").textContent = hotkeyName;
  if (pictureStyle) {
    const choices = Object.entries(options.style || {});
    stableHTML(pictureStyle, choices.map(([name, value]) => `<option value="${esc(value)}">${esc(name)}</option>`).join(""));
    pictureStyle.disabled = !choices.length;
    pictureStyle.value = settings.style || "";
  }
  $("#lab-model-label").textContent = cloudActive ? (missingKey ? "API key needed" : "Cloud API") : cloudSelected ? "Cloud disabled" : "Local model";
  if (box && sel) $(`#${box} ${sel}`)?.focus(); // the chip row is rebuilt when a value changes; keep keyboard focus on the same control
}
pictureStyle?.addEventListener("change", event => setSetting("style", event.target.value));
async function setSetting(k, v) {
  if (k === "cloud_enabled" && v && !await askConfirm(
    "Prompt Lab and voice-triggered prompt engineering text, instructions, and relevant preferences will be sent to the configured API provider. Improve can make multiple paid calls. Estimates are not your bill.",
    { title: "Allow paid cloud AI?", confirmLabel: "Allow paid cloud" }
  )) return false;
  const value = k === "microphone" && v === "" ? null : v;
  const promptModelChange = k === "engine_local_model" || k === "judge_local_model";
  const speechChange = k === "speech_model";
  if(speechChange) {
    if(speechModelSavePending)return false;
    speechModelSavePending=true; speechOptionsVersion++;
    const speechStatus = $("#speech-model-status");
    if (speechStatus) speechStatus.textContent = "Changing speech model…";
    if (modelCatalog) renderModelCatalog(modelCatalog);
  }
  const previewChange=k==="live_preview";
  if(previewChange) {previewSettingVersion++;previewActionPending++;settings.live_preview=!!value;invalidatePreview();}
  const settingPreviewVersion=previewSettingVersion;
  if(k === "local_model") modelChoiceUnconfirmed = true;
  let savedOk = false;
  try {
    try {
      const saved = await api().set_setting(k, value);
      savedOk = !!saved;
      if (!saved) {
        const msg = speechChange ? "Speech model change did not complete. Check the selected model and speech status; finish any current recording or task before trying again."
          : k === "local_model" ? "Model choice could not be saved. Refresh the list and choose an available Ollama model."
          : promptModelChange ? "That Ollama model could not be selected. Refresh the local models and try again."
          : k === "microphone" ? "Microphone could not be changed. Finish any recording or reconnect the selected microphone."
          : k === "command_hotkey" ? "Command Mode hotkey could not be saved. Choose a different key than your dictation hotkey."
          : "Setting could not be saved. Finish any current recording and try again.";
        toast(msg, true);
      }
      if(k === "local_model") { modelChoiceUnconfirmed = false; if(saved) settings.local_model = value; }
    } catch (e) { toast(speechChange ? "Speech model change was not confirmed. Check the selected model and speech status before continuing." : k === "local_model" ? "Model choice was not confirmed. Refresh the list to check your saved choice." : previewChange?"Preview setting was not confirmed. Check Settings before continuing.":"Setting could not be saved. Your previous choice is preserved.", true); }
    const latest=await api().get_settings();
    if (speechChange && api().get_options) {
      const prevMics = options.microphone;
      options = await api().get_options();
      if (prevMics) options.microphone = prevMics;
    }
    if(k === "local_model") modelChoiceUnconfirmed = false;
    if(previewChange&&settingPreviewVersion!==previewSettingVersion)return savedOk;
    settings=settingPreviewVersion===previewSettingVersion?latest:{...latest,live_preview:settings.live_preview};
    renderChips();
    if (k === "hotkey") setStatus(S);
    if (page === "settings") {
      syncSettings();
      if (k === "microphone") await populateMicrophones();
      if (["cleanup", "cloud_enabled", "local_model", "speech_model"].includes(k)) {
        refreshSettingsDetails();
        loadModelCatalog();
      }
    }
    return savedOk;
  } catch {
    toast("Setting status could not be refreshed. Reopen Settings to check your choice.",true);
    return false;
  } finally {
    if(previewChange) {previewActionPending--;invalidatePreview();}
    if(speechChange) {
      speechModelSavePending=false; speechOptionsVersion++;
      const speechStatus = $("#speech-model-status");
      if (speechStatus) speechStatus.textContent = "";
      if (modelCatalog) renderModelCatalog(modelCatalog);
    }
  }
}

// ---------- settings page ----------
const SETTING_ROWS = [
  ["hotkey", "Dictation hotkey", "Press to start or finish, or hold and release.", "hotkey"],
  ["command_hotkey", "Command Mode hotkey", "Highlight text in any app, press this key, and speak an edit."],
  ["microphone", "Microphone", "Choose a microphone for your next recording."],
  ["destination", "After dictation", "Send finished text to the active app or Prompt Lab."],
  ["live_preview", "Live preview", "See words as you speak on this computer.", "toggle"],
  ["floating_card", "Floating card", "Show the small indicator while recording.", "toggle"],
  ["sounds", "Sound cues", "Play subtle audio cues on start and finish.", "toggle"],
  ["restore_clipboard", "Restore clipboard", "Put your previous clipboard item back after pasting.", "toggle"],
  ["quiet_mode", "Quiet / whisper mode", "Boosts soft speech and lowers the silence threshold.", "toggle"],
  ["speech_model", "Speech to text", "Turns your voice into raw text on CPU.", "speech"],
  ["cleanup", "AI formatting", "Removes fillers, obeys spoken commands, and structures lists or paragraphs locally.", "toggle"],
  ["style", "Formatting style", "Choose whether Listen keeps your exact wording or tightens rambles."],
  ["auto_app_tone", "App-aware tone", "Adapts tone automatically for chat, email, and code apps.", "toggle"],
  ["local_model", "AI formatting model (Ollama)", "Formats and structures dictation on this computer.", "model"],
  ["target", "Prompt for", "Target assistant for Prompt Lab drafts."],
  ["engine", "Writing AI", "Writes and improves Prompt Lab drafts."],
  ["judge", "Checking AI", "Scores your prompt, then suggests a better version."],
  ["engine_local_model", "Local model for writing", "", "prompt-model"],
  ["judge_local_model", "Local model for checking", "", "prompt-model"],
  ["judge_model", "Claude model for checking", "Used only when Checking AI is Claude and paid cloud is on."],
  ["cloud_enabled", "Allow paid cloud AI", "Off keeps all prompts local. Everyday dictation formatting always stays on this PC.", "toggle"],
];
const SETTINGS_FILTER_KEYWORDS = {
  general: "general dictation hotkey command mode edit highlight key mic microphone audio destination delivery paste live preview floating card sound cues clipboard restore quiet whisper soft behavior",
  models: "voice ai speech text parakeet whisper turbo medium small cpu formatting format cleanup clean style tighten keep wording app aware tone casual polished technical rules slack email code ollama local model qwen gemma llama download install update benchmark test speed vram",
  snippets: "snippets voice trigger expand expansion phrase template abbreviation email link signature macro",
  appearance: "appearance theme color scheme warm charcoal paper midnight sage high contrast espresso nordic plum ocean obsidian mist blossom layout writer desk sidebar compact look",
  lab: "prompt lab cloud paid api key claude openai gemini anthropic writing checking target engineer local model",
  system: "system shortcuts keyboard keybind saved files clear drafts instructions logs diagnostics copy details troubleshooting",
};
let speechModelSavePending=false, speechOptionsVersion=0, speechOptionsError="", settingsRendered=false, localModelsAvailable=false, modelsRefreshPending=false, speechRuntimeBusy=false;
let modelCatalog=null, modelsAutoChecked=false, modelCheckPending=false;
let settingsTab="general", speechListOpen=false, cleanupListOpen=false, toneRulesOpen=false;
let snippetsList=[], appTonesData={ auto_enabled: true, presets: [["casual","Casual"],["polished","Polished"],["technical","Technical"],["default","Default"]], rules: [] };
let installPollTimer=0, lastInstallFinishedRun=0, installWasActive=false, installPollErrors=0;
let benchPollTimer=0, lastBenchFinishedRun=0, benchWasActive=false, benchPollErrors=0;
const swHTML = on => `<span class="sw-t" aria-hidden="true">${on ? "On" : "Off"}</span>`;
function setSwitch(b, on) {
  b.classList.toggle("on", on); b.setAttribute("aria-checked", String(on));
  const t = b.querySelector(".sw-t"); if (t) t.textContent = on ? "On" : "Off";
}
function commandHotkeyOptions() {
  const current = String(settings.command_hotkey || "");
  const dictKey = String(settings.hotkey || "");
  const entries = optionEntries("hotkey");
  let html = `<option value=""${!current ? " selected" : ""}>Off (not set)</option>`;
  let seenCurrent = !current;
  for (const [name, val] of entries) {
    const isDict = val === dictKey;
    if (val === current) seenCurrent = true;
    html += `<option value="${esc(val)}"${val === current ? " selected" : ""}${isDict ? " disabled" : ""}>${esc(name)}${isDict ? " · dictation key" : ""}</option>`;
  }
  if (!seenCurrent) {
    html += `<option value="${esc(current)}" selected>${esc(hotkeyLabel(current))}</option>`;
  }
  return html;
}
function setSettingsTab(tab, opts = {}) {
  const valid = ["general", "models", "snippets", "appearance", "lab", "system"];
  settingsTab = valid.includes(tab) ? tab : "general";
  const targetBtn = $(`#settings-nav [data-settings-tab="${settingsTab}"]`);
  const filterInput = $("#settings-filter");
  if (targetBtn?.hidden && filterInput && filterInput.value) {
    filterInput.value = "";
    applySettingsFilter("");
  }
  const stage = $("#settings-stage");
  if (stage && (!filterInput || !filterInput.value.trim())) stage.hidden = false;
  $$("#settings-nav [data-settings-tab]").forEach(btn => {
    const active = btn.dataset.settingsTab === settingsTab;
    if (active) btn.setAttribute("aria-current", "page");
    else btn.removeAttribute("aria-current");
  });
  $$("[data-settings-pane]").forEach(pane => {
    pane.classList.toggle("is-active", pane.dataset.settingsPane === settingsTab);
  });
  if (settingsTab === "models") {
    loadAppTones();
    if (!modelsAutoChecked) {
      modelsAutoChecked = true;
      if (!opts.skipAutoCheck) checkForBetterModels(true);
    }
  }
  if (settingsTab === "snippets") {
    loadSnippets();
  }
}
function syncToneRulesDrawer() {
  const wrap = $("#tone-rules-wrap"), btn = $("#btn-toggle-tone-rules");
  if (!wrap || !btn) return;
  wrap.hidden = !toneRulesOpen;
  btn.setAttribute("aria-expanded", String(toneRulesOpen));
  const count = Array.isArray(appTonesData?.rules) ? appTonesData.rules.length : 0;
  btn.textContent = toneRulesOpen ? `Hide rules (${count})` : `Custom rules (${count})`;
}
function toneLabel(id) {
  const key = String(id || "default").toLowerCase();
  const found = (appTonesData?.presets || []).find(([v]) => v === key);
  return found ? found[1] : key.charAt(0).toUpperCase() + key.slice(1);
}
function renderAppTones(data) {
  if (data && typeof data === "object") {
    appTonesData = {
      auto_enabled: data.auto_enabled ?? !!settings.auto_app_tone,
      presets: Array.isArray(data.presets) && data.presets.length ? data.presets : appTonesData.presets,
      rules: Array.isArray(data.rules) ? data.rules : [],
    };
  }
  const select = $("#tone-preset-select");
  if (select && Array.isArray(appTonesData.presets)) {
    const prev = select.value || "casual";
    const html = appTonesData.presets.map(([val, label]) => `<option value="${esc(val)}">${esc(label)}</option>`).join("");
    stableHTML(select, html);
    if ([...select.options].some(o => o.value === prev)) select.value = prev;
  }
  const list = $("#tone-rules-list");
  if (list) {
    const rules = appTonesData.rules || [];
    if (!rules.length) {
      list.innerHTML = `<p class="setting-help">No custom rules yet. Built-in defaults apply automatically for chat, email, and code apps.</p>`;
    } else {
      list.innerHTML = rules.map(rule => `<div class="catalog-item tone-rule-item">
        <div class="catalog-info">
          <div class="catalog-head"><strong>${esc(rule.app)}</strong><span class="route-tag">${esc(toneLabel(rule.tone))}</span></div>
        </div>
        <div class="catalog-action"><button type="button" class="btn sm quiet" data-delete-tone="${esc(rule.app)}" aria-label="Remove tone rule for ${esc(rule.app)}">Remove</button></div>
      </div>`).join("");
      $$("[data-delete-tone]", list).forEach(btn => btn.onclick = () => deleteAppToneRule(btn.dataset.deleteTone));
    }
  }
  syncToneRulesDrawer();
}
async function loadAppTones() {
  if (!$("#tone-rules-list")) return;
  try {
    if (typeof api().get_app_tones === "function") {
      const data = await api().get_app_tones();
      renderAppTones(data);
    } else {
      renderAppTones({ auto_enabled: !!settings.auto_app_tone, rules: Array.isArray(settings.app_tone_rules) ? settings.app_tone_rules : [] });
    }
  } catch { /* keep last rendered rules */ }
}
async function addAppToneRule() {
  const appInput = $("#tone-app-input"), presetSelect = $("#tone-preset-select"), addBtn = $("#btn-add-tone-rule");
  if (!appInput || !presetSelect) return;
  const app = appInput.value.trim(), tone = presetSelect.value || "casual";
  if (!app) { toast("Enter an app name first (e.g. Slack, Outlook, VS Code).", true); appInput.focus(); return; }
  if (addBtn) addBtn.disabled = true;
  try {
    const res = await api().add_app_tone(app, tone);
    if (res?.ok) {
      appInput.value = "";
      if (Array.isArray(res.rules)) renderAppTones({ ...appTonesData, rules: res.rules });
      else await loadAppTones();
      toast(`Tone rule saved for ${app}.`);
      appInput.focus();
    } else {
      toast(res?.message || "Could not save app tone rule.", true);
    }
  } catch {
    toast("Could not save app tone rule.", true);
  } finally {
    if (addBtn) addBtn.disabled = false;
  }
}
async function deleteAppToneRule(app) {
  try {
    const res = await api().delete_app_tone(app);
    if (res?.ok) {
      if (Array.isArray(res.rules)) renderAppTones({ ...appTonesData, rules: res.rules });
      else await loadAppTones();
      toast(`Removed tone rule for ${app}.`);
    } else {
      toast(res?.message || "Could not remove tone rule.", true);
    }
  } catch {
    toast("Could not remove tone rule.", true);
  }
}
function renderSnippets(list) {
  snippetsList = Array.isArray(list) ? list : [];
  const box = $("#snippets-list");
  if (!box) return;
  if (!snippetsList.length) {
    box.innerHTML = `<div class="inset-row"><span class="muted">No snippets yet. Add a spoken trigger above to expand phrases automatically during dictation.</span></div>`;
    return;
  }
  box.innerHTML = snippetsList.map(item => `<div class="inset-row snippet-row">
    <div class="snippet-body">
      <span class="snippet-trigger-pill">${esc(item.trigger)}</span>
      <span class="snippet-arrow" aria-hidden="true">→</span>
      <span class="snippet-expansion-text">${esc(item.expansion)}</span>
    </div>
    <div class="setting-controls"><button type="button" class="btn sm quiet" data-delete-snippet="${esc(item.trigger)}" aria-label="Remove snippet ${esc(item.trigger)}">Remove</button></div>
  </div>`).join("");
  $$("[data-delete-snippet]", box).forEach(btn => btn.onclick = () => deleteSnippet(btn.dataset.deleteSnippet));
}
async function loadSnippets() {
  if (!$("#snippets-list")) return;
  try {
    if (typeof api().get_snippets === "function") {
      const list = await api().get_snippets();
      renderSnippets(list);
    } else {
      renderSnippets(settings.snippets || []);
    }
  } catch { /* keep last rendered snippets */ }
}
async function addSnippet() {
  const triggerEl = $("#snippet-trigger"), expEl = $("#snippet-expansion"), btn = $("#btn-add-snippet"), status = $("#snippet-form-status");
  if (!triggerEl || !expEl) return;
  const trigger = triggerEl.value.trim(), expansion = expEl.value;
  if (!trigger) {
    if (status) status.textContent = "Enter a spoken trigger phrase.";
    triggerEl.focus();
    return;
  }
  if (!expansion.trim()) {
    if (status) status.textContent = "Enter the expansion text to insert.";
    expEl.focus();
    return;
  }
  if (status) status.textContent = "";
  if (btn) btn.disabled = true;
  try {
    const res = await api().add_snippet(trigger, expansion);
    if (res?.ok) {
      triggerEl.value = "";
      expEl.value = "";
      if (Array.isArray(res.snippets)) renderSnippets(res.snippets);
      else await loadSnippets();
      toast(`Saved snippet “${trigger}”.`);
      triggerEl.focus();
    } else {
      const msg = res?.message || "Could not save snippet.";
      if (status) status.textContent = msg;
      toast(msg, true);
    }
  } catch {
    toast("Could not save snippet.", true);
  } finally {
    if (btn) btn.disabled = false;
  }
}
async function deleteSnippet(trigger) {
  try {
    const res = await api().delete_snippet(trigger);
    if (res?.ok) {
      if (Array.isArray(res.snippets)) renderSnippets(res.snippets);
      else await loadSnippets();
      toast(`Removed snippet “${trigger}”.`);
    } else {
      toast(res?.message || "Could not remove snippet.", true);
    }
  } catch {
    toast("Could not remove snippet.", true);
  }
}
function syncModelDrawers() {
  const sWrap = $("#speech-catalog-wrap"), sBtn = $("#btn-toggle-speech-models");
  if (sWrap && sBtn) {
    sWrap.hidden = !speechListOpen;
    sBtn.setAttribute("aria-expanded", String(speechListOpen));
    const count = Array.isArray(modelCatalog?.speech) ? modelCatalog.speech.length : 0;
    sBtn.textContent = speechListOpen ? "Hide models" : (count ? `Switch / install (${count})` : "Switch / install");
  }
  const cWrap = $("#cleanup-catalog-wrap"), cBtn = $("#btn-toggle-cleanup-models");
  if (cWrap && cBtn) {
    cWrap.hidden = !cleanupListOpen;
    cBtn.setAttribute("aria-expanded", String(cleanupListOpen));
    const count = Array.isArray(modelCatalog?.cleanup) ? modelCatalog.cleanup.length : 0;
    cBtn.textContent = cleanupListOpen ? "Hide models" : (count ? `Switch / install (${count})` : "Switch / install");
  }
}
function filterHaystackFor(tab) {
  let hay = SETTINGS_FILTER_KEYWORDS[tab] || tab;
  if (tab === "models" && modelCatalog) {
    const names = [
      ...(modelCatalog.speech || []).map(m => `${m.id || ""} ${m.name || ""}`),
      ...(modelCatalog.cleanup || []).map(m => `${m.id || ""} ${m.name || ""}`),
    ];
    if (names.length) hay += " " + names.join(" ").toLowerCase();
  }
  return hay;
}
function applySettingsFilter(rawQuery) {
  const q = String(rawQuery || "").trim().toLowerCase();
  const terms = q ? q.split(/\s+/).filter(Boolean) : [];
  const appearanceUnavailable = $("#appearance")?.hidden && !window.listenDashboard;
  const navButtons = $$("#settings-nav [data-settings-tab]");
  const matched = [];
  for (const btn of navButtons) {
    const tab = btn.dataset.settingsTab;
    if (tab === "appearance" && appearanceUnavailable) { btn.hidden = true; continue; }
    const hay = filterHaystackFor(tab);
    const ok = !terms.length || terms.every(t => hay.includes(t));
    btn.hidden = !ok;
    if (ok) matched.push(tab);
  }
  const empty = $("#settings-filter-empty"), stage = $("#settings-stage");
  if (empty) empty.hidden = matched.length > 0;
  if (stage) stage.hidden = matched.length === 0;
  if (matched.length && !matched.includes(settingsTab)) {
    setSettingsTab(matched[0]);
  }
  if (q && settingsTab === "models") {
    if (/parakeet|whisper|turbo|speech/.test(q) && !speechListOpen) { speechListOpen = true; syncModelDrawers(); }
    if (/ollama|qwen|gemma|llama|benchmark|format|clean/.test(q) && !cleanupListOpen) { cleanupListOpen = true; syncModelDrawers(); }
    if (/tone|casual|polished|technical|rule/.test(q) && !toneRulesOpen) { toneRulesOpen = true; syncToneRulesDrawer(); }
  }
}
// Built once when Settings opens. Later visits sync values in place so active category and focus are preserved.
let microphoneOptionsVersion=0;
async function populateMicrophones() {
  const select = $('#settings-grid select[data-key="microphone"]');
  if (!select || !api().get_microphones) return;
  const version=++microphoneOptionsVersion;
  try {
    const microphones=await api().get_microphones();
    if(version!==microphoneOptionsVersion||select!==$('#settings-grid select[data-key="microphone"]'))return;
    const saved=settings.microphone ?? null;
    options.microphone=microphones;
    select.innerHTML=Object.entries(microphones).map(([name,value])=>`<option value="${esc(value??'')}"${saved===value?' selected':''}>${esc(name)}</option>`).join('');
    const missing=saved!=null&&!Object.values(microphones).includes(saved);
    if(missing)select.add(new Option('Saved microphone not connected',saved,true,true));
    const note = $("#microphone-note");
    note.hidden = !missing;
    $("#use-default-microphone").hidden = !missing;
    select.setAttribute('aria-invalid',String(missing));
    if (missing) select.setAttribute('aria-describedby', 'microphone-note');
    else select.removeAttribute('aria-describedby');
    note.querySelector('p').innerHTML='<strong>Saved microphone not connected.</strong> Listen is still set to this microphone. Connect it again or choose another one.';
  } catch {
    if(version!==microphoneOptionsVersion||select!==$('#settings-grid select[data-key="microphone"]'))return;
    const saved = settings.microphone ?? null;
    if (saved != null && ![...select.options].some(o => o.value === saved)) {
      const opt = new Option(`${saved} (saved; could not check)`, saved, true, true);
      opt.disabled = true;
      select.add(opt);
    }
    const note = $("#microphone-note");
    note.hidden = false;
    $("#use-default-microphone").hidden = true;
    note.querySelector('p').textContent='Could not check your microphones. Your saved choice is kept. Try opening Settings again.';
  }
}
function refreshSpeechOptions() {
  if (!api().get_options || speechModelSavePending) return;
  const version = speechOptionsVersion;
  api().get_options().then(latest => {
    if (speechModelSavePending || version !== speechOptionsVersion) return;
    speechOptionsError = "";
    const prevMics = options.microphone;
    options = latest;
    if (prevMics) options.microphone = prevMics;
    syncSettings();
    loadModelCatalog();
  }).catch(() => {
    if (version === speechOptionsVersion && !speechModelSavePending) {
      speechOptionsError = "Could not check the speech AI list. Your current choice is kept.";
      const status = $("#speech-model-status");
      if (status) status.textContent = speechOptionsError;
    }
  });
}
function renderSettings() {
  if (settingsRendered && $("#settings-grid").children.length) {
    syncSettings();
    populateMicrophones();
    refreshSettingsDetails();
    if (!localModelsAvailable && !modelsRefreshPending) refreshModels();
    refreshSpeechOptions();
    loadModelCatalog();
    loadSnippets();
    loadAppTones();
    return;
  }
  settingsRendered = true;
  const rows = new Map(SETTING_ROWS.map(row => [row[0], row]));
  const controlId = k => k === "hotkey" ? "settings-hotkey" : k === "speech_model" ? "speech-model" : k === "local_model" ? "local-model" : k === "style" ? "cleanup-style" : "setting-" + k;
  const control = (key, withDesc = false) => {
    const [k, l, , t] = rows.get(key);
    const id = controlId(k), descAttr = withDesc ? ` aria-describedby="desc-${k}"` : "";
    return k === "hotkey" ? `<button type="button" class="btn" id="${id}" data-key="hotkey" aria-haspopup="dialog" aria-controls="hotkey-dialog"${descAttr}></button>`
      : k === "command_hotkey" ? `<select id="${id}" data-key="command_hotkey"${descAttr}>${commandHotkeyOptions()}</select>`
      : t === "toggle" ? `<button type="button" class="switch ${settings[k] ? "on" : ""}" id="${id}" data-toggle="${k}" role="switch" aria-checked="${!!settings[k]}" aria-labelledby="label-${k}"${descAttr}>${swHTML(!!settings[k])}</button>`
      : t === "prompt-model" ? `<select id="${id}" data-key="${k}" aria-label="${esc(l)}">${promptLocalChoices(k)}</select>`
      : `<select id="${id}" data-key="${k}"${k === "judge_model" || k === "style" ? ` aria-label="${esc(l)}"` : ""}${descAttr}>${optionEntries(k).map(([n, v]) => `<option value="${esc(v)}"${settings[k] === v ? " selected" : ""}>${esc(n)}</option>`).join("")}</select>`;
  };
  const compactRow = (k, showHelp = false) => {
    const [, label, description] = rows.get(k);
    const id = controlId(k);
    return `<div class="inset-row"><div><label class="l" id="label-${k}" for="${id}">${esc(label)}</label>${showHelp ? `<div class="d" id="desc-${k}">${esc(description)}</div>` : ""}</div><div class="setting-controls">${control(k, showHelp)}</div></div>`;
  };
  const toggleCell = (k, showHelp = false) => {
    const [, label, description] = rows.get(k);
    const id = controlId(k);
    return showHelp
      ? `<div class="toggle-cell"><div><label class="l" id="label-${k}" for="${id}">${esc(label)}</label><div class="d" id="desc-${k}">${esc(description)}</div></div>${control(k, true)}</div>`
      : `<div class="toggle-cell"><label class="l" id="label-${k}" for="${id}">${esc(label)}</label>${control(k, false)}</div>`;
  };
  $("#settings-grid").innerHTML = `
    <section class="settings-pane" data-settings-pane="general" aria-labelledby="general-heading">
      <div class="settings-pane-head">
        <h2 id="general-heading">General</h2>
      </div>
      <div class="settings-inset">
        ${compactRow("hotkey")}
        ${compactRow("command_hotkey", true)}
        ${compactRow("microphone")}
        <div class="microphone-note" id="microphone-note" role="status" hidden><p><strong>Saved microphone not connected.</strong> Listen is still set to this microphone. Connect it again or choose another one.</p><button type="button" class="btn" id="use-default-microphone">Use Windows default</button></div>
        ${compactRow("destination")}
      </div>
      <div class="settings-group-block">
        <h3 class="settings-eyebrow" id="behavior-heading">Behavior</h3>
        <div class="settings-toggle-grid" role="group" aria-labelledby="behavior-heading">
          ${toggleCell("live_preview")}
          ${toggleCell("floating_card")}
          ${toggleCell("sounds")}
          ${toggleCell("restore_clipboard")}
          ${toggleCell("quiet_mode", true)}
        </div>
      </div>
    </section>

    <section class="settings-pane" data-settings-pane="models" aria-labelledby="models-heading">
      <div class="settings-pane-head">
        <h2 id="models-heading">Voice &amp; AI</h2>
        <div class="actions">
          <button type="button" class="btn sm quiet" id="btn-check-models">Check updates</button>
          <button type="button" class="btn sm" id="btn-run-benchmark">Run benchmark</button>
        </div>
      </div>
      <p class="model-check-note" id="model-check-note" role="status"></p>
      <div class="install-bar" id="model-install-bar" hidden>
        <div class="install-bar-head">
          <strong id="model-install-title">Downloading…</strong>
          <span class="setting-help" id="model-install-meta"></span>
          <button type="button" class="btn sm quiet" id="btn-cancel-install">Cancel</button>
        </div>
        <progress id="model-install-progress" max="100" value="0"></progress>
      </div>
      <div class="install-bar" id="model-benchmark-bar" hidden>
        <div class="install-bar-head">
          <strong id="model-benchmark-title">Running benchmark…</strong>
          <span class="setting-help" id="model-benchmark-meta"></span>
          <button type="button" class="btn sm quiet" id="btn-cancel-benchmark">Cancel</button>
        </div>
        <progress id="model-benchmark-progress" max="100" value="0"></progress>
      </div>

      <div class="settings-inset">
        <div class="inset-row">
          <div>
            <div class="inset-row-title">
              <span class="l">Speech to text</span>
              <span class="route-tag active" id="speech-active-badge">Parakeet 0.6B</span>
            </div>
            <div class="d" id="speech-stage-summary">Local CPU · 0 GB VRAM</div>
            <div class="d" id="speech-model-status" role="status"></div>
          </div>
          <div class="setting-controls">
            <button type="button" class="btn sm" id="btn-toggle-speech-models" aria-expanded="false" aria-controls="speech-catalog-wrap">Switch / install</button>
          </div>
        </div>
        <div class="catalog-drawer" id="speech-catalog-wrap" hidden>
          <div class="catalog-list" id="speech-catalog-list"></div>
        </div>
      </div>

      <div class="settings-inset">
        <div class="inset-row">
          <div>
            <label class="l" id="label-cleanup" for="setting-cleanup">AI formatting</label>
            <div class="d" id="desc-cleanup">Removes fillers, obeys spoken commands, and structures lists or paragraphs locally.</div>
          </div>
          <div class="setting-controls">
            ${control("style", false)}
            ${control("cleanup", true)}
          </div>
        </div>
        <div class="inset-row" id="cleanup-model-row">
          <div>
            <div class="inset-row-title">
              <span class="l">Formatting model</span>
              <span class="route-tag active" id="cleanup-active-badge">Ollama</span>
            </div>
            <div class="d" id="model-status" role="status">Checking Ollama models…</div>
          </div>
          <div class="setting-controls">
            <button type="button" class="btn sm" id="btn-toggle-cleanup-models" aria-expanded="false" aria-controls="cleanup-catalog-wrap">Switch / install</button>
          </div>
        </div>
        <div class="catalog-drawer" id="cleanup-catalog-wrap" hidden>
          <div class="catalog-drawer-head">
            <span class="setting-help">Ollama models on this computer</span>
            <button type="button" class="btn sm quiet" id="refresh-models">Refresh list</button>
          </div>
          <div class="catalog-list" id="cleanup-catalog-list"></div>
        </div>
      </div>

      <div class="settings-inset">
        <div class="inset-row">
          <div>
            <label class="l" id="label-auto_app_tone" for="setting-auto_app_tone">App-aware tone</label>
            <div class="d" id="desc-auto_app_tone">Adapts tone automatically for chat, email, and code apps.</div>
          </div>
          <div class="setting-controls">
            <button type="button" class="btn sm quiet" id="btn-toggle-tone-rules" aria-expanded="false" aria-controls="tone-rules-wrap">Custom rules (0)</button>
            ${control("auto_app_tone", true)}
          </div>
        </div>
        <div class="catalog-drawer" id="tone-rules-wrap" hidden>
          <div class="catalog-drawer-head">
            <span class="setting-help">Override tone for specific apps (e.g. Slack, Outlook, VS Code).</span>
          </div>
          <div class="tone-add-row">
            <label class="sr-only" for="tone-app-input">App name</label>
            <input id="tone-app-input" class="input" type="text" placeholder="App name (e.g. Slack, Outlook)" autocomplete="off" maxlength="80">
            <label class="sr-only" for="tone-preset-select">Tone preset</label>
            <select id="tone-preset-select">
              <option value="casual">Casual</option>
              <option value="polished">Polished</option>
              <option value="technical">Technical</option>
              <option value="default">Default</option>
            </select>
            <button type="button" class="btn sm primary" id="btn-add-tone-rule">Add rule</button>
          </div>
          <div class="tone-rules-list" id="tone-rules-list"></div>
        </div>
      </div>

      <div class="model-benchmarks">
        <button type="button" class="link benchmark-toggle" id="btn-benchmarks" aria-expanded="false" aria-controls="benchmark-panel">Benchmark results</button>
        <div id="benchmark-panel" class="benchmark-panel" hidden>
          <p id="model-benchmark-summary"></p>
          <div id="model-benchmark-content"></div>
        </div>
      </div>
    </section>

    <section class="settings-pane" data-settings-pane="snippets" aria-labelledby="snippets-heading">
      <div class="settings-pane-head">
        <h2 id="snippets-heading">Snippets</h2>
      </div>
      <p class="setting-help settings-pane-intro">Say a trigger phrase during dictation to insert its full text automatically.</p>
      <div class="settings-inset">
        <div class="snippet-form">
          <div class="snippet-field">
            <label for="snippet-trigger">Spoken trigger</label>
            <input id="snippet-trigger" class="input" type="text" placeholder="e.g. my email" autocomplete="off">
          </div>
          <div class="snippet-field">
            <label for="snippet-expansion">Expansion text</label>
            <textarea id="snippet-expansion" class="input" rows="2" placeholder="e.g. name@example.com"></textarea>
          </div>
          <div class="snippet-form-foot">
            <span class="setting-help" id="snippet-form-status" role="status"></span>
            <button type="button" class="btn sm primary" id="btn-add-snippet">Add snippet</button>
          </div>
        </div>
      </div>
      <div class="settings-group-block">
        <h3 class="settings-eyebrow" id="saved-snippets-heading">Saved snippets</h3>
        <div class="settings-inset" id="snippets-list" role="region" aria-labelledby="saved-snippets-heading"></div>
      </div>
    </section>

    <section class="settings-pane" data-settings-pane="lab" aria-labelledby="lab-settings-heading">
      <div class="settings-pane-head">
        <h2 id="lab-settings-heading">Prompt Lab</h2>
        <button type="button" class="btn sm quiet" id="refresh-prompt-models">Refresh local models</button>
      </div>
      <div class="settings-inset">
        ${compactRow("cloud_enabled", true)}
        ${compactRow("target")}
      </div>
      <div class="settings-inset">
        <div class="route-stack">
          <div class="route-stack-head">
            <label class="l" id="label-engine" for="setting-engine">Writing AI</label>
            <div class="setting-controls">${control("engine")}${control("engine_local_model")}</div>
          </div>
          <p data-route-summary="engine"></p>
          <div class="key-help" data-key-help="engine" hidden></div>
        </div>
        <div class="route-stack">
          <div class="route-stack-head">
            <label class="l" id="label-judge" for="setting-judge">Checking AI</label>
            <div class="setting-controls">${control("judge")}${control("judge_local_model")}${control("judge_model", true)}</div>
          </div>
          <p data-route-summary="judge"></p>
          <p id="desc-judge_model">${esc(rows.get("judge_model")[2])}</p>
          <div class="key-help" data-key-help="judge" hidden></div>
        </div>
      </div>
      <p class="setting-help" id="prompt-model-status" role="status"></p>
    </section>`;

  $$("#settings-nav [data-settings-tab]").forEach(btn => {
    btn.onclick = () => setSettingsTab(btn.dataset.settingsTab);
  });
  const filterInput = $("#settings-filter");
  if (filterInput) {
    filterInput.oninput = () => applySettingsFilter(filterInput.value);
    filterInput.onkeydown = e => {
      if (e.key === "Escape" && filterInput.value) {
        e.stopPropagation();
        filterInput.value = "";
        applySettingsFilter("");
      }
    };
  }
  $$("#settings-grid select[data-key]").forEach(s => s.onchange = () => setSetting(s.dataset.key, s.value));
  $$("#settings-grid .switch").forEach(b => b.onclick = () => setSetting(b.dataset.toggle, !settings[b.dataset.toggle]));
  $("#settings-hotkey").onclick = event => openHotkeyPicker(event.currentTarget);
  $("#btn-toggle-speech-models").onclick = () => { speechListOpen = !speechListOpen; syncModelDrawers(); };
  $("#btn-toggle-cleanup-models").onclick = () => { cleanupListOpen = !cleanupListOpen; syncModelDrawers(); };
  $("#btn-toggle-tone-rules").onclick = () => { toneRulesOpen = !toneRulesOpen; syncToneRulesDrawer(); };
  $("#btn-add-tone-rule").onclick = addAppToneRule;
  $("#tone-app-input").addEventListener("keydown", ev => { if (ev.key === "Enter" && !ev.isComposing && ev.keyCode !== 229) { ev.preventDefault(); addAppToneRule(); } });
  $("#btn-add-snippet").onclick = addSnippet;
  $("#snippet-trigger").addEventListener("keydown", ev => { if (ev.key === "Enter" && !ev.isComposing && ev.keyCode !== 229) { ev.preventDefault(); $("#snippet-expansion").focus(); } });
  $("#snippet-expansion").addEventListener("keydown", ev => { if (ev.key === "Enter" && ev.ctrlKey && !ev.isComposing && ev.keyCode !== 229) { ev.preventDefault(); addSnippet(); } });
  populateMicrophones();
  $("#use-default-microphone").onclick = async () => {
    await setSetting("microphone", "");
    if (page === "settings" && settings.microphone == null) {
      $('#settings-grid select[data-key="microphone"]')?.focus();
    }
  };
  $("#refresh-models").onclick = $("#refresh-prompt-models").onclick = refreshModels;
  refreshModels();
  const benchBtn = $("#btn-benchmarks"), benchPanel = $("#benchmark-panel");
  benchBtn.onclick = () => {
    const open = benchPanel.hidden;
    benchPanel.hidden = !open;
    benchBtn.setAttribute("aria-expanded", String(open));
    if (open) benchPanel.scrollIntoView({ block: "nearest" });
  };
  $("#btn-check-models").onclick = () => checkForBetterModels(false);
  $("#btn-cancel-install").onclick = async () => {
    try { await api().cancel_install?.(); } catch {}
  };
  $("#btn-run-benchmark").onclick = () => startBenchmark("");
  $("#btn-cancel-benchmark").onclick = async (ev) => {
    ev.currentTarget.disabled = true;
    try { await api().cancel_benchmark?.(); } catch {}
  };
  const appearanceBtn = $('#settings-nav [data-settings-tab="appearance"]');
  if (appearanceBtn && $("#appearance")?.hidden && !window.listenDashboard) appearanceBtn.hidden = true;
  setSettingsTab(settingsTab);
  syncModelDrawers();
  syncToneRulesDrawer();
  refreshSpeechOptions();
  syncSettings();
  refreshSettingsDetails();
  loadModelCatalog();
  loadSnippets();
  loadAppTones();
}
function fmtBytes(n) {
  if (!Number.isFinite(n) || n <= 0) return "";
  if (n >= 1024 * 1024 * 1024) return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  return `${Math.round(n / (1024 * 1024))} MB`;
}
function renderInstallBar(st) {
  const bar = $("#model-install-bar");
  if (!bar) return;
  if (!st || !st.active) {
    bar.hidden = true;
    return;
  }
  bar.hidden = false;
  $("#model-install-title").textContent = st.status || `Installing ${st.label || "model"}…`;
  const prog = $("#model-install-progress");
  if (Number.isFinite(st.percent)) {
    prog.value = st.percent;
    const sizeText = st.total > 0 ? `${fmtBytes(st.downloaded)} / ${fmtBytes(st.total)} · ` : "";
    $("#model-install-meta").textContent = `${sizeText}${st.percent}%`;
  } else {
    prog.removeAttribute("value");
    $("#model-install-meta").textContent = st.downloaded > 0 ? fmtBytes(st.downloaded) : "Working…";
  }
}
function renderBenchmarkBar(st) {
  const bar = $("#model-benchmark-bar");
  if (!bar) return;
  if (!st || !st.active) {
    bar.hidden = true;
    const cancelBtn = $("#btn-cancel-benchmark");
    if (cancelBtn) cancelBtn.disabled = false;
    return;
  }
  bar.hidden = false;
  $("#model-benchmark-title").textContent = st.status || "Running benchmark…";
  const prog = $("#model-benchmark-progress");
  if (Number.isFinite(st.percent)) {
    prog.value = st.percent;
    $("#model-benchmark-meta").textContent = `${st.percent}%`;
  } else {
    prog.removeAttribute("value");
    $("#model-benchmark-meta").textContent = "Working…";
  }
}
function renderBenchmarkTable(data) {
  const summary = $("#model-benchmark-summary"), detail = $("#model-benchmark-content"), btn = $("#btn-benchmarks");
  if (!summary || !detail) return;
  const rows = Object.values(data?.benchmarks || {});
  if (btn) btn.textContent = rows.length ? `Benchmark results (${rows.length})` : "Benchmark results";
  if (!rows.length) {
    summary.textContent = "No benchmarks have been run on this build yet.";
    detail.innerHTML = "<p>Click <strong>Run benchmark</strong> above (or <strong>Test</strong> on any installed model) to test your installed local Ollama models on 12 real dictation samples (fillers, repetitions, lists, file paths, commands, and latency).</p>";
    return;
  }
  rows.sort((a, b) => (b.passed / (b.cases || 1)) - (a.passed / (a.cases || 1)) || a.median_seconds - b.median_seconds);
  summary.textContent = `Measured on this computer across ${rows.length} ${rows.length === 1 ? "model" : "models"} (12 dictation formatting samples each, warm median latency).`;
  detail.innerHTML = `<div class="benchmark-table-wrap"><table><thead><tr><th scope="col">Model</th><th scope="col">Passed</th><th scope="col">Median time</th><th scope="col">Style</th><th scope="col">Tested</th></tr></thead><tbody>${rows.map(r => {
    const pct = r.cases ? Math.round(100 * r.passed / r.cases) : 0;
    const styleLabel = r.style === "clean" ? "Keep wording" : "Tighten & format";
    return `<tr><th scope="row">${esc(r.model)}</th><td>${r.passed}/${r.cases} · ${pct}%</td><td>${Number(r.median_seconds).toFixed(2)} s</td><td>${styleLabel}</td><td>${esc(r.recorded || "")}</td></tr>`;
  }).join("")}</tbody></table></div><p>Scores reflect Listen's current formatter and safety checks. Larger models use more GPU VRAM — balance score, speed, and memory headroom for your workflow.</p>`;
}
function captureCatalogFocus(container) {
  const active = document.activeElement;
  if (!active || !container || !container.contains(active)) return null;
  for (const attr of ["data-use-speech", "data-install-speech", "data-use-cleanup", "data-install-cleanup", "data-bench-cleanup"]) {
    const val = active.getAttribute?.(attr);
    if (val != null) return { attr, val };
  }
  return null;
}
function restoreCatalogFocus(container, saved) {
  if (!saved || !container) return;
  const exact = [...container.querySelectorAll(`[${saved.attr}]`)].find(el => el.getAttribute(saved.attr) === saved.val && !el.disabled);
  if (exact) exact.focus();
}
function renderModelCatalog(data) {
  if (!data) return;
  modelCatalog = data;
  const installing = !!data.install?.active;
  const benching = !!data.benchmark?.active;
  const busy = installing || benching;
  renderInstallBar(data.install);
  renderBenchmarkBar(data.benchmark);
  renderBenchmarkTable(data);
  if (data.check_note) {
    const note = $("#model-check-note");
    if (note) note.textContent = data.check_note;
  }
  const hasInstalledCleanup = Array.isArray(data.cleanup) && data.cleanup.some(item => item.installed);
  const runBenchBtn = $("#btn-run-benchmark");
  if (runBenchBtn) runBenchBtn.disabled = !data.ollama_available || !hasInstalledCleanup || busy || modelSavePending;

  const speechBox = $("#speech-catalog-list");
  if (speechBox && Array.isArray(data.speech)) {
    const speechFocus = captureCatalogFocus(speechBox);
    const activeSpeech = settings.speech_model || options.speech_model || "";
    const activeSpeechItem = data.speech.find(item => item.id === activeSpeech);
    const speechBadge = $("#speech-active-badge");
    if (speechBadge && activeSpeechItem) speechBadge.textContent = activeSpeechItem.name;
    const speechSum = $("#speech-stage-summary");
    if (speechSum && activeSpeechItem) speechSum.textContent = `${activeSpeechItem.ram_note} · ${activeSpeechItem.tag}`;
    speechBox.innerHTML = data.speech.map(item => {
      const isActive = item.id === activeSpeech;
      const action = isActive
        ? `<span class="catalog-badge active">Active</span>`
        : item.installed
          ? `<button type="button" class="btn sm" data-use-speech="${esc(item.id)}"${speechModelSavePending || speechRuntimeBusy || installing ? " disabled" : ""}>Use</button>`
          : item.downloadable
            ? `<button type="button" class="btn sm" data-install-speech="${esc(item.id)}"${busy ? " disabled" : ""}>Download (~${esc(item.size_mb)} MB)</button>`
            : `<span class="catalog-badge">Not found</span>`;
      return `<div class="catalog-item${isActive ? " is-active" : ""}">
        <div class="catalog-info">
          <div class="catalog-head"><strong>${esc(item.name)}</strong><span class="route-tag">${esc(item.tag)}</span></div>
          <div class="catalog-meta">${esc(item.ram_note)} · ${esc(item.note)}</div>
        </div>
        <div class="catalog-action">${action}</div>
      </div>`;
    }).join("");
    $$("[data-use-speech]", speechBox).forEach(b => b.onclick = () => setSetting("speech_model", b.dataset.useSpeech));
    $$("[data-install-speech]", speechBox).forEach(b => b.onclick = () => startModelInstall("speech", b.dataset.installSpeech));
    restoreCatalogFocus(speechBox, speechFocus);
  }

  const cleanupBox = $("#cleanup-catalog-list");
  if (cleanupBox && Array.isArray(data.cleanup)) {
    const cleanupFocus = captureCatalogFocus(cleanupBox);
    const activeCleanup = settings.local_model || "";
    const cleanupOn = !!settings.cleanup;
    const ollamaOk = !!data.ollama_available;
    const activeCleanupItem = data.cleanup.find(item => item.id === activeCleanup);
    const activeMissing = !!activeCleanup && !activeCleanupItem?.installed;
    const cleanupBadge = $("#cleanup-active-badge");
    if (cleanupBadge) {
      cleanupBadge.textContent = !activeCleanup
        ? (cleanupOn ? "Not selected" : "Off")
        : activeMissing
          ? `${activeCleanup} (${ollamaOk ? "not installed" : "offline"})`
          : (cleanupOn ? activeCleanup : `${activeCleanup} (off)`);
      cleanupBadge.classList.toggle("active", cleanupOn && !!activeCleanup && !activeMissing);
    }
    const modelStatus = $("#model-status");
    if (modelStatus && !modelsRefreshPending) {
      const installedCount = data.cleanup.filter(item => item.installed).length;
      if (!ollamaOk) {
        modelStatus.textContent = "Ollama unavailable · Start Ollama and refresh";
      } else if (activeMissing) {
        modelStatus.textContent = `Saved model (${activeCleanup}) is not installed in Ollama · ${installedCount} installed`;
      } else {
        const parts = [`Ollama · ${installedCount} installed`];
        if (activeCleanupItem?.vram_note) parts.push(activeCleanupItem.vram_note);
        if (activeCleanupItem?.benchmark?.label) parts.push(`Measured ${activeCleanupItem.benchmark.label}`);
        modelStatus.textContent = parts.join(" · ");
      }
    }
    const installedList = data.cleanup.filter(item => item.installed || item.id === activeCleanup);
    const availableList = data.cleanup.filter(item => !item.installed && item.id !== activeCleanup && item.downloadable);
    const renderRow = item => {
      const isSelected = item.id === activeCleanup;
      const scoreText = item.benchmark?.label ? `Measured: ${item.benchmark.label}` : "Not benchmarked yet";
      const meta = [scoreText, item.note].filter(Boolean).join(" · ");
      const testBtn = item.installed && ollamaOk
        ? `<button type="button" class="btn sm quiet" data-bench-cleanup="${esc(item.id)}"${busy || modelSavePending ? " disabled" : ""}>Test</button>`
        : "";
      const primaryAction = isSelected && item.installed
        ? `<span class="catalog-badge${cleanupOn ? " active" : ""}">${cleanupOn ? "Active" : "Selected (off)"}</span>`
        : isSelected && !item.installed
          ? `<span class="catalog-badge">${ollamaOk ? "Saved · not installed" : "Saved · offline"}</span>${item.downloadable ? `<button type="button" class="btn sm" data-install-cleanup="${esc(item.id)}"${!ollamaOk || busy ? " disabled" : ""}>Download (${esc(item.size_gb)})</button>` : ""}`
          : item.installed
            ? `<button type="button" class="btn sm" data-use-cleanup="${esc(item.id)}"${busy || modelSavePending ? " disabled" : ""}>Use</button>`
            : `<button type="button" class="btn sm" data-install-cleanup="${esc(item.id)}"${!ollamaOk || busy ? " disabled" : ""}>Download (${esc(item.size_gb)})</button>`;
      return `<div class="catalog-item${isSelected && item.installed && cleanupOn ? " is-active" : ""}">
        <div class="catalog-info">
          <div class="catalog-head"><strong>${esc(item.name)}</strong><span class="route-tag">${esc(item.tier)}</span></div>
          <div class="catalog-meta">${esc(meta)}</div>
        </div>
        <div class="catalog-action">${testBtn}${primaryAction}</div>
      </div>`;
    };
    let html = installedList.length
      ? installedList.map(renderRow).join("")
      : `<p class="setting-help">${ollamaOk ? "No local models installed in Ollama yet. Download one below." : "Ollama is not reachable. Start Ollama and click Refresh list."}</p>`;
    if (availableList.length) {
      html += `<div class="catalog-subhead">Available to download</div>${availableList.map(renderRow).join("")}`;
    }
    cleanupBox.innerHTML = html;
    $$("[data-use-cleanup]", cleanupBox).forEach(b => b.onclick = () => useCleanupModel(b.dataset.useCleanup));
    $$("[data-install-cleanup]", cleanupBox).forEach(b => b.onclick = () => startModelInstall("cleanup", b.dataset.installCleanup));
    $$("[data-bench-cleanup]", cleanupBox).forEach(b => b.onclick = () => startBenchmark(b.dataset.benchCleanup));
    restoreCatalogFocus(cleanupBox, cleanupFocus);
  }
  syncModelDrawers();
}
async function useCleanupModel(id) {
  if (modelSavePending) return;
  modelSavePending = true;
  if (modelCatalog) renderModelCatalog(modelCatalog);
  try {
    const wasOff = !settings.cleanup;
    const saved = await setSetting("local_model", id);
    if (saved && wasOff) {
      const turnedOn = await setSetting("cleanup", true);
      toast(turnedOn ? `Switched to ${id} and turned AI formatting on.` : `Switched model to ${id}, but AI formatting could not be enabled.`, !turnedOn);
    } else if (saved) {
      toast(`Switched AI formatting to ${id}.`);
    }
  } finally {
    modelSavePending = false;
    if (modelCatalog) renderModelCatalog(modelCatalog);
    await refreshModels();
  }
}
async function loadModelCatalog() {
  if (!api().get_model_catalog || !$("#speech-catalog-list")) return;
  try {
    const data = await api().get_model_catalog();
    installWasActive = !!data.install?.active;
    benchWasActive = !!data.benchmark?.active;
    renderModelCatalog(data);
    if (data.install?.active) scheduleInstallPoll();
    if (data.benchmark?.active) scheduleBenchPoll();
  } catch {}
}
async function checkForBetterModels(gentle = false) {
  const btn = $("#btn-check-models"), note = $("#model-check-note");
  if (!api().check_model_updates || modelCheckPending) return;
  modelCheckPending = true;
  if (btn) btn.disabled = true;
  if (note) note.textContent = "Checking local Ollama models and latest whisper.cpp releases…";
  try {
    const data = await api().check_model_updates();
    installWasActive = !!data.install?.active;
    benchWasActive = !!data.benchmark?.active;
    renderModelCatalog(data);
    if (!modelsRefreshPending) refreshModels();
    if (data.install?.active) scheduleInstallPoll();
    if (data.benchmark?.active) scheduleBenchPoll();
    if (!gentle) toast("Model check complete.");
  } catch {
    if (note) note.textContent = "Could not check online right now. Local model list is still available.";
    if (!gentle) toast("Model check timed out or offline. Local options remain available.", true);
  } finally {
    modelCheckPending = false;
    if (btn) btn.disabled = false;
  }
}
async function startModelInstall(kind, id) {
  if (!api().install_model) return;
  try {
    const res = await api().install_model(kind, id);
    if (!res?.ok) {
      toast(res?.message || "Could not start download.", true);
      return;
    }
    installPollErrors = 0;
    toast(res.message || "Starting download…");
    scheduleInstallPoll(true);
  } catch {
    toast("Could not start download. Check your connection and try again.", true);
  }
}
function scheduleInstallPoll(immediate = false) {
  clearTimeout(installPollTimer);
  installPollTimer = setTimeout(pollInstallStatus, immediate ? 50 : 400);
}
async function pollInstallStatus() {
  if (!api().get_install_status) return;
  try {
    const st = await api().get_install_status();
    installPollErrors = 0;
    renderInstallBar(st);
    if (st.active) {
      if (!installWasActive) {
        installWasActive = true;
        if (modelCatalog) { modelCatalog.install = st; renderModelCatalog(modelCatalog); }
      }
      scheduleInstallPoll();
      return;
    }
    const wasActive = installWasActive;
    installWasActive = false;
    const runId = Number(st.run_id) || 0;
    if (runId > 0 && runId !== lastInstallFinishedRun && (st.done || st.error)) {
      lastInstallFinishedRun = runId;
      if (st.done) {
        toast(st.status || "Model installed.");
        refreshSpeechOptions();
        await refreshModels();
        await loadModelCatalog();
      } else if (st.error) {
        toast(st.error, true);
        await loadModelCatalog();
      }
    } else if (wasActive) {
      await loadModelCatalog();
    }
  } catch {
    installPollErrors++;
    if (installPollErrors <= 5) {
      scheduleInstallPoll();
    } else {
      installWasActive = false;
      renderInstallBar(null);
      loadModelCatalog();
    }
  }
}
async function startBenchmark(targetModel = "") {
  if (!api().start_benchmark) return;
  try {
    const res = await api().start_benchmark(targetModel);
    if (!res?.ok) {
      toast(res?.message || "Could not start benchmark.", true);
      return;
    }
    benchPollErrors = 0;
    toast(targetModel ? `Benchmarking ${targetModel}…` : "Running formatting benchmark on installed models…");
    scheduleBenchPoll(true);
  } catch {
    toast("Could not start benchmark.", true);
  }
}
function scheduleBenchPoll(immediate = false) {
  clearTimeout(benchPollTimer);
  benchPollTimer = setTimeout(pollBenchmarkStatus, immediate ? 50 : 500);
}
async function pollBenchmarkStatus() {
  if (!api().get_benchmark_status) return;
  try {
    const st = await api().get_benchmark_status();
    benchPollErrors = 0;
    renderBenchmarkBar(st);
    if (st.active) {
      if (!benchWasActive) {
        benchWasActive = true;
        if (modelCatalog) { modelCatalog.benchmark = st; renderModelCatalog(modelCatalog); }
      }
      scheduleBenchPoll();
      return;
    }
    const wasActive = benchWasActive;
    benchWasActive = false;
    const runId = Number(st.run_id) || 0;
    if (runId > 0 && runId !== lastBenchFinishedRun && (st.done || st.error)) {
      lastBenchFinishedRun = runId;
      if (st.done) {
        toast(st.status || "Benchmark complete.");
        await loadModelCatalog();
        const panel = $("#benchmark-panel"), btn = $("#btn-benchmarks");
        if (panel && btn) { panel.hidden = false; btn.setAttribute("aria-expanded", "true"); }
      } else if (st.error) {
        toast(st.error, true);
        await loadModelCatalog();
      }
    } else if (wasActive) {
      await loadModelCatalog();
    }
  } catch {
    benchPollErrors++;
    if (benchPollErrors <= 5) {
      scheduleBenchPoll();
    } else {
      benchWasActive = false;
      renderBenchmarkBar(null);
      loadModelCatalog();
    }
  }
}
function refreshSettingsDetails() {
  return api().diagnostics().then(d => {
    const labels={speech:"Speech to text","clean-up":"AI formatting","prompt lab":"Prompt Lab",files:"Saved files"};
    $("#diag").innerHTML = Object.entries(d).map(([k,v])=>`<div class="diag-row"><dt>${esc(labels[k]||k)}</dt><dd${k==="speech"?' id="diag-speech"':""}>${esc(v)}</dd></div>`).join("");
  }).catch(()=>{$('#diag').innerHTML='<div class="diag-row"><dt>Status</dt><dd>Could not read the app details. Open Settings again to try.</dd></div>';});
}
async function copySafeText(text, successMsg) {
  if (!text) { toast("Nothing to copy yet.", true); return; }
  try {
    if (typeof api().copy_text === "function") {
      if (await api().copy_text(text)) toast(successMsg);
      else toast("Could not copy right now. Resolve any pending clipboard recovery first.", true);
    } else if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      toast(successMsg);
    } else {
      toast("Could not copy.", true);
    }
  } catch { toast("Could not copy.", true); }
}
$("#btn-copy-diag")?.addEventListener("click", () => {
  const text = $$("#diag .diag-row").map(row => `${$("dt", row)?.textContent || ""}: ${$("dd", row)?.textContent || ""}`).join("\n");
  copySafeText(text, "Details copied.");
});
async function recheckCloudKeys() {
  if (!api().get_options) return;
  try {
    const prevMics = options.microphone;
    options = await api().get_options();
    if (prevMics) options.microphone = prevMics;
    renderChips();
    syncSettings();
    toast("Checked Windows environment variables.");
  } catch { toast("Could not re-check API keys.", true); }
}
function renderKeyHelp(container, stage) {
  const route = settings[stage];
  const provider = route && route !== "local" ? options.prompt_providers?.[route] : null;
  if (!provider || provider.configured) {
    container.hidden = true;
    stableHTML(container, "");
    return;
  }
  const primaryVar = String(provider.key_env || "").split(/\s+or\s+/i)[0].trim();
  const cmd = `setx ${primaryVar} "your-api-key"`;
  container.hidden = false;
  stableHTML(container, `<p><strong>Missing ${esc(provider.name)} API key.</strong> In Windows Terminal, run <code>${esc(cmd)}</code>, then restart Listen (or re-check after setting it).</p><div class="actions"><button type="button" class="btn sm" data-copy-env="${esc(primaryVar)}">Copy variable name</button><button type="button" class="btn sm quiet" data-recheck-keys>Re-check keys</button></div>`);
  const copyBtn = $("[data-copy-env]", container);
  if (copyBtn) copyBtn.onclick = () => copySafeText(primaryVar, `Copied ${primaryVar}`);
  const recheckBtn = $("[data-recheck-keys]", container);
  if (recheckBtn) recheckBtn.onclick = recheckCloudKeys;
}
function syncSettings() {
  const staticSelectKeys = new Set(["destination", "style", "target", "engine", "judge", "judge_model"]);
  $$("#settings-grid select[data-key]").forEach(s => {
    const key = s.dataset.key;
    if (key === "command_hotkey") {
      stableHTML(s, commandHotkeyOptions());
    } else if (staticSelectKeys.has(key) && options[key]) {
      const html = optionEntries(key).map(([n, v]) => `<option value="${esc(v)}">${esc(n)}</option>`).join("");
      if (html) stableHTML(s, html);
    }
    const v = String(settings[key] ?? "");
    if (s.value !== v && [...s.options].some(o => o.value === v)) s.value = v;
  });
  const hotkeyButton = $("#settings-hotkey");
  if (hotkeyButton) {
    const label = hotkeyLabel(settings.hotkey);
    hotkeyButton.textContent = label;
    hotkeyButton.setAttribute("aria-label", "Change dictation hotkey, " + label);
    const shortcutKey = $("#shortcut-dictate");
    if (shortcutKey) shortcutKey.textContent = label;
  }
  const shortcutCmd = $("#shortcut-command");
  if (shortcutCmd) shortcutCmd.textContent = settings.command_hotkey ? hotkeyLabel(settings.command_hotkey) : "Off";
  $$("#settings-grid .switch").forEach(b => setSwitch(b, !!settings[b.dataset.toggle]));
  const cleanupOn = !!settings.cleanup;
  const cleanupRow = $("#cleanup-model-row");
  if (cleanupRow) cleanupRow.classList.toggle("cleanup-row-disabled", !cleanupOn);
  const styleSelect = $("#cleanup-style");
  if (styleSelect) styleSelect.disabled = !cleanupOn;
  const refreshLocalBtn = $("#refresh-models");
  if (refreshLocalBtn && !modelSavePending && !modelsRefreshPending) refreshLocalBtn.disabled = false;

  const speechBadge = $("#speech-active-badge");
  if (speechBadge && !modelCatalog?.speech) {
    speechBadge.textContent = (options.speech_model_label || settings.speech_model || "Parakeet").replace(/\s*\(default\)/i, "");
  }
  const cleanupBadge = $("#cleanup-active-badge");
  if (cleanupBadge) {
    const modelName = settings.local_model || "";
    cleanupBadge.textContent = cleanupOn ? (modelName || "Ollama") : (modelName ? `${modelName} (off)` : "Off");
    cleanupBadge.classList.toggle("active", cleanupOn && !!modelName);
  }

  $$("#settings-grid [data-route-summary]").forEach(el => {
    const stage = el.dataset.routeSummary;
    const key = stage + "_local_model";
    const local = $('#settings-grid select[data-key="' + key + '"]');
    if (local) {
      local.hidden = settings[stage] !== "local";
      stableHTML(local, promptLocalChoices(key));
      local.value = settings[key] || "";
    }
    el.textContent = promptRouteSummary(stage);
    const help = $(`#settings-grid [data-key-help="${stage}"]`);
    if (help) renderKeyHelp(help, stage);
  });
  const refresh = $("#refresh-prompt-models");
  if (refresh) {
    refresh.hidden = settings.engine !== "local" && settings.judge !== "local";
    if (!modelsRefreshPending) refresh.disabled = false;
  }
  const cloudJudge = $('#settings-grid select[data-key="judge_model"]');
  if (cloudJudge) cloudJudge.hidden = settings.judge !== "anthropic";
  $('#desc-judge_model')?.classList.toggle('hidden', settings.judge !== "anthropic");
}
let modelsRefreshVersion = 0, modelSavePending = false, modelChoiceUnconfirmed = false;
async function refreshModels() {
  const list = $("#cleanup-catalog-list"), status = $("#model-status"), button = $("#refresh-models");
  const promptBtn = $("#refresh-prompt-models"), promptStatus = $("#prompt-model-status");
  if (!list) return;
  if (modelSavePending) {
    if (status) status.textContent = "Saving model choice…";
    if (promptStatus) promptStatus.textContent = "Saving model choice…";
    return;
  }
  const version = ++modelsRefreshVersion;
  const current = () => version === modelsRefreshVersion && list === $("#cleanup-catalog-list");
  modelsRefreshPending = true;
  if (button) button.disabled = true;
  if (promptBtn) promptBtn.disabled = true;
  if (status) status.textContent = "Checking models installed in Ollama…";
  if (promptStatus) promptStatus.textContent = "Checking Ollama models…";
  try {
    const data = await api().get_local_models(); if (!current()) return;
    if (data.selected) { settings.local_model = data.selected; modelChoiceUnconfirmed = false; }
    const names = [...new Set((data.models || []).filter(n => typeof n === "string" && n))];
    rememberPromptModels(data);
    const available = data.available === true || (data.available !== false && (names.length > 0 || !data.error));
    localModelsAvailable = available && names.length > 0;
    if (status) status.textContent = data.error || (names.length ? `${names.length} local ${names.length === 1 ? "model" : "models"} installed in Ollama.` : "No models installed in Ollama yet.");
    if (promptStatus) promptStatus.textContent = data.error ? "Ollama unavailable." : names.length ? `${names.length} local ${names.length === 1 ? "model" : "models"} found.` : "No local models installed in Ollama.";
    modelsRefreshPending = false;
    syncSettings();
    loadModelCatalog();
  } catch {
    if (!current()) return;
    modelsRefreshPending = false;
    localModelsAvailable = false;
    rememberPromptModels({models:[],available:false});
    syncSettings();
    loadModelCatalog();
    if (status) status.textContent = modelChoiceUnconfirmed ? "Model list unavailable. Reconnect and refresh to check your choice." : "Ollama unavailable. Start Ollama and click Refresh list.";
    if (promptStatus) promptStatus.textContent = "Could not reach Ollama.";
  } finally {
    if (current()) {
      modelsRefreshPending = false;
      if (button) button.disabled = false;
      if (promptBtn) promptBtn.disabled = false;
    }
  }
}

// ---------- vocabulary suggestions ----------
async function loadSuggestions() {
  const words = await api().get_vocab_suggestions();
  const previous = document.activeElement, action = previous?.dataset?.add !== undefined ? "add" : previous?.dataset?.no !== undefined ? "no" : null;
  const box = $("#vocab-suggest"); box.classList.toggle("hidden", !words.length);
  $("#vocab-chips").innerHTML = words.map(w => `<span class="chip" data-w="${esc(w)}">${esc(w)}<button class="add" title="Add to vocabulary" aria-label="Add ${esc(w)} to vocabulary" data-add="${esc(w)}">+</button><button class="no" title="Don't suggest again" aria-label="Stop suggesting ${esc(w)}" data-no="${esc(w)}">✕</button></span>`).join("");
  if (action) ($$("#vocab-chips [data-" + action + "]").find(b => b.dataset[action] === previous.dataset[action]) || vocab).focus();
  $$("#vocab-chips [data-add]").forEach(b => { b.disabled = vocabAdding || quitPending; b.onclick = () => addVocabSuggestion(b.dataset.add); });
  $$("#vocab-chips [data-no]").forEach(b => b.onclick = async () => { try { if(await api().dismiss_vocab(b.dataset.no)) await loadSuggestions(); else toast("Suggestion removal was not confirmed.", true); } catch { toast("Suggestion removal was not confirmed. Try again.", true); } });
}

// ---------- history & stats ----------
function fmtTs(ts) { const d = new Date(ts); return d.toLocaleDateString(undefined, { month: "short", day: "numeric" }) + " " + d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" }); }
function wordLabel(n) { return n + (n === 1 ? " word" : " words"); }  // spelled out: a bare "w" reads as weeks
function historyInboxRow(e, i, app = "", time = fmtTs(e.ts)) {
  const uncertain = e.delivery_status === "uncertain" || e.delivery_status === "pasted";
  const retained = e.delivery_status === "retained";
  const unknown = deliveryNeedsCheck(e) && !uncertain;
  const delivery = uncertain ? "Paste not confirmed" : retained ? "Not sent" : e.delivery_status === "copied" ? "Copied for manual paste" : e.delivery_status === "verified" ? "Paste confirmed" : unknown ? "Paste status unknown" : "Text sent · arrival not checked";
  const next = uncertain || unknown ? "Check the original app first. The text may already be there. Copy again only if it is missing." : retained ? "This text was saved instead of being sent. Copy it into the app where you want it." : e.delivery_status === "copied" ? "Paste into your intended app when you are ready." : "Saved here if you need to read or reuse it.";
  const label = uncertain || unknown ? "Copy after checking" : retained ? "Copy saved text" : "Copy text";
  return `<li class="history-entry${uncertain || unknown ? " needs-attention" : ""}" data-i="${i}" data-delivery="${esc(e.delivery_status)}"><div class="history-meta">${app}<time datetime="${esc(e.ts)}">${esc(time)}</time><span>${e.kind==="engineer"?"Engineered prompt":e.kind==="prompt"?"Prompt":"Dictation"}</span><span class="history-saved">Text saved</span>${e.by?`<span>${esc(e.by)}</span>`:""}<span class="history-count">${wordLabel(e.text.trim().split(/\s+/).filter(Boolean).length)}${e.cost?` · $${e.cost<.01?e.cost.toFixed(4):e.cost.toFixed(2)}`:""}</span></div><p class="history-text">${esc(e.text)}</p><div class="history-delivery"><strong>${delivery}</strong><span>${next}</span></div><div class="history-actions"><button class="btn sm" data-history-copy="clean">${label}</button>${e.raw?'<button class="btn sm quiet" data-history-copy="raw">Copy original</button>':""}${uncertain && e.clipboard_recovery !== false?'<button class="link" data-review-paste>Clipboard recovery</button>':""}</div></li>`;
}
function histRow(e, i) {
  const outcome = { verified: "Paste confirmed", uncertain: "Check original app before copying again", retained: "Not sent - recover in Listen", copied: "Copied - paste manually", sent: "Text sent; arrival not checked", pasted: "Paste attempted; check original app", unknown: "Delivery status unavailable" }[e.delivery_status] || "";
  return `<li data-i="${i}"><span class="t">${fmtTs(e.ts)}</span>${e.kind === "prompt" ? '<span class="badge">prompt</span>' : e.kind === "engineer" ? '<span class="badge">Engineered prompt</span>' : ""}<span class="x">${esc(e.text)}${e.by ? `<small class="muted"> · ${esc(e.by)}</small>` : ""}${outcome ? `<small class="delivery-outcome">${outcome}</small>` : ""}</span><button class="btn sm" data-history-copy="clean">Copy</button>${e.raw ? '<button class="btn sm" data-history-copy="raw">Copy original</button>' : ""}<span class="n" title="Word count">${wordLabel(e.text.trim().split(/\s+/).filter(Boolean).length)}${e.cost ? ` · $${e.cost < 0.01 ? e.cost.toFixed(4) : e.cost.toFixed(2)}` : ""}</span></li>`;
}
// Rows keep their entries in memory; a row finds its entry by index instead of carrying copies of the text in the markup.
function deliveryNeedsCheck(entry) {
  return !["verified", "sent", "copied", "retained"].includes(entry.delivery_status);
}
function bindRows(ul, rows) {
  $$("[data-history-copy]",ul).forEach(button=>button.onclick=async()=>{
    const entry=rows[+button.closest('li').dataset.i];
    if(!entry)return;
    try {
      if (api().get_delivery_status && (await api().get_delivery_status()).pending) {
        await showPasteRecovery();
        return;
      }
      if(deliveryNeedsCheck(entry)){
        if(!await askConfirm('Check the original app first. This text may already be there. Copy again only if you checked and it is missing.'))return;
      }
      const text=button.dataset.historyCopy==='raw'?(entry.raw||''):entry.text;
      if(await api().copy_text(text))toast('Copied. Paste it where you want it.');
      else {toast('Copy was not confirmed. Check recovery before trying again.',true);await loadDeliveryStatus();}
    } catch {toast('Copy was not confirmed. Your text stays saved here.',true);}
  });
  $$('[data-review-paste]', ul).forEach(button => button.onclick = async () => {
    try { await showPasteRecovery(); }
    catch { toast("Could not check clipboard recovery. Your text stays saved here.", true); }
  });
}
async function showPasteRecovery() {
  await loadDeliveryStatus();
  if($('#delivery-recovery').classList.contains('hidden')){toast('No paste is waiting for clipboard recovery. Check the original app before copying again.');return;}
  setBannersOpen(true);
  $('.main').scrollTo({top:0,behavior:matchMedia('(prefers-reduced-motion: reduce)').matches?'instant':'smooth'});
}

const HIST_PAGE = 200;
let histAll = [], histShown = [], recentShown = [], histLimit = HIST_PAGE, histTimer;
let historyMode = "day", historyCurrent, historyJumpTop, historyScrollFrame;
const historyCollapsed = new Set();
const historyDayLabel = new Intl.DateTimeFormat(undefined, {weekday:"long", month:"long", day:"numeric", year:"numeric"});
const historyShortDay = new Intl.DateTimeFormat(undefined, {month:"short", day:"numeric"});
const historyShortOldDay = new Intl.DateTimeFormat(undefined, {month:"short", day:"numeric", year:"2-digit"});
const historyTime = new Intl.DateTimeFormat(undefined, {hour:"2-digit", minute:"2-digit"});
const sharpHistoryIcon = icon => typeof icon === "string" && icon.startsWith("iVBORw0KGgoAAAANSUhEUgAAAIAAAACA");

function historyAppName(app) {
  return typeof app?.name === "string" && app.name.trim() && app.name.length <= 80 ? app.name : "";
}
function historyAppMark(app) {
  const name = historyAppName(app);
  if (!name) return '<span class="history-app unknown">App not recorded</span>';
  const icon = app.icon;
  const mark = typeof icon === "string" && icon.length <= 24576 && /^iVBORw0KGgo[A-Za-z0-9+/]*={0,2}$/.test(icon)
    ? `<img src="data:image/png;base64,${icon}" alt="" width="22" height="22">`
    : `<span class="history-app-mark" aria-hidden="true">${esc(Array.from(name)[0].toUpperCase())}</span>`;
  return `<span class="history-app">${mark}${esc(name)}</span>`;
}
function historyGroups(rows, byApp) {
  const groups = new Map();
  const year = new Date().getFullYear();
  for (const row of rows) {
    const date = new Date(row.ts), dated = Number.isFinite(date.getTime()), app = historyAppName(row.app);
    const key = byApp ? "app:" + app.toLowerCase() : dated ? `day:${date.getFullYear()}-${date.getMonth()}-${date.getDate()}` : "day:unknown";
    let group = groups.get(key);
    if (!group) {
      const label = byApp ? app || "App not recorded" : dated ? historyDayLabel.format(date) : "Date unavailable";
      const short = byApp || !dated ? label : (date.getFullYear() === year ? historyShortDay : historyShortOldDay).format(date);
      group = {key, label, short, app:row.app, entries:[]};
      groups.set(key, group);
    }
    group.entries.push(row);
    if (row.app?.icon && (!group.app?.icon || (sharpHistoryIcon(row.app.icon) && !sharpHistoryIcon(group.app.icon)))) group.app = row.app;
  }
  const ordered = [...groups.values()];
  if (byApp && groups.has("app:")) ordered.push(...ordered.splice(ordered.indexOf(groups.get("app:")), 1));
  return ordered;
}
function selectHistoryGroup(index) {
  const next = $(`#history-jump-list [data-jump="${index}"]`);
  if (!next || next.getAttribute("aria-current") === "location") return;
  $("#history-jump-list [aria-current]")?.removeAttribute("aria-current");
  next.setAttribute("aria-current", "location");
  historyCurrent = next.dataset.historyKey;
  const rail = next.parentElement, item = next.getBoundingClientRect(), visible = rail.getBoundingClientRect();
  if (item.top < visible.top) rail.scrollTop -= visible.top - item.top;
  else if (item.bottom > visible.bottom) rail.scrollTop += item.bottom - visible.bottom;
}
function renderHistory() {
  const q = $("#hist-search").value.trim().toLowerCase();
  const rows = histAll.filter(e => !q || [e.text, e.raw || "", historyAppName(e.app) || "App not recorded"].some(value => value.toLowerCase().includes(q)));
  const byApp = historyMode === "app", groups = historyGroups(rows, byApp);
  const list = $("#hist-list"), rail = $("#history-jump"), jumps = $("#history-jump-list");
  const focused = document.activeElement;
  const focusedJump = focused?.closest("#history-jump-list a")?.dataset.historyKey;
  const focusedGroup = focused?.closest(".history-group");
  const focusedEntry = focused?.closest(".history-entry");
  const previousEntry = focusedEntry && histShown[+focusedEntry.dataset.i];
  let rendered = "", start = 0;
  histShown = [];
  for (const [index, group] of groups.entries()) {
    group.start = start;
    start += group.entries.length;
    if (group.start >= histLimit) continue;
    const heading = byApp ? historyAppMark(group.app) : esc(group.label);
    const entries = group.entries.slice(0, histLimit - group.start).map(entry => {
      const date = new Date(entry.ts);
      const time = byApp || !Number.isFinite(date.getTime()) ? fmtTs(entry.ts) : historyTime.format(date);
      return historyInboxRow(entry, histShown.push(entry) - 1, byApp ? "" : historyAppMark(entry.app), time);
    });
    rendered += `<details class="history-group" id="history-group-${index}" data-history-key="${esc(group.key)}"${historyCollapsed.has(group.key) ? "" : " open"}><summary>${heading}<small>${group.entries.length} ${group.entries.length === 1 ? "entry" : "entries"}</small></summary><ul class="list history-entries">${entries.join("")}</ul></details>`;
  }
  if (!groups.some(group => group.start < histLimit && group.key === historyCurrent)) historyCurrent = groups[0]?.key;
  rail.hidden = !groups.length;
  rail.setAttribute("aria-label", byApp ? "Jump to an app" : "Jump to a date");
  jumps.innerHTML = groups.map((group, index) => `<a href="#history-group-${index}" data-jump="${index}" data-history-key="${esc(group.key)}" aria-label="${esc(group.label)}, ${group.entries.length} ${group.entries.length === 1 ? "entry" : "entries"}"${group.key === historyCurrent ? ' aria-current="location"' : ""}><span>${esc(group.short)}</span><span>${group.entries.length}</span></a>`).join("");
  list.innerHTML = rendered ? rendered + (rows.length > histLimit ? `<button class="btn" id="hist-more">Show more (${rows.length - histLimit} more)</button>` : "")
    : q ? '<p class="history-empty">No matching entries. <button class="link" id="clear-filter">Clear filter</button></p>' : '<p class="history-empty">Your dictations will appear here after you record.</p>';
  $("#history-matches").textContent = `${rows.length} ${rows.length === 1 ? "entry" : "entries"}`;
  $$(".history-group", list).forEach(group => group.ontoggle = () => {
    if (!group.isConnected) return;
    if (group.open) historyCollapsed.delete(group.dataset.historyKey);
    else historyCollapsed.add(group.dataset.historyKey);
  });
  $$("[data-jump]", jumps).forEach(link => link.onclick = event => {
    event.preventDefault();
    const index = +link.dataset.jump, group = groups[index];
    if (group.start >= histLimit) {
      histLimit = (Math.floor(group.start / HIST_PAGE) + 1) * HIST_PAGE;
      renderHistory();
    }
    const detail = $(`#history-group-${index}`);
    detail.open = true;
    detail.querySelector("summary").focus({preventScroll:true});
    detail.scrollIntoView({block:"start"});
    historyJumpTop = $(".main").scrollTop;
    selectHistoryGroup(index);
  });
  bindRows(list, histShown);
  if($("#clear-filter")) $("#clear-filter").onclick = () => { $("#hist-search").value=""; histLimit=HIST_PAGE; renderHistory(); $("#hist-search").focus(); };
  if($("#hist-more")) $("#hist-more").onclick = () => {
    const next = histLimit; histLimit += HIST_PAGE; renderHistory();
    ($("#hist-more") || $(`#hist-list [data-i="${next}"] [data-history-copy]`))?.focus();
  };
  if (focusedGroup) {
    const summary = $$(".history-group", list).find(group => group.dataset.historyKey === focusedGroup.dataset.historyKey)?.querySelector("summary");
    const index = previousEntry ? histShown.findIndex(entry => entry.ts === previousEntry.ts && entry.kind === previousEntry.kind && entry.text === previousEntry.text) : -1;
    const action = focused.dataset.historyCopy ? `[data-history-copy="${focused.dataset.historyCopy}"]` : "[data-review-paste]";
    ((index >= 0 && $(`#hist-list [data-i="${index}"] ${action}`)) || summary)?.focus({preventScroll:true});
  } else if (focusedJump) $$("[data-jump]", jumps).find(link => link.dataset.historyKey === focusedJump)?.focus({preventScroll:true});
}
$$('[data-history-mode]').forEach(button => button.onclick = () => {
  historyMode = button.dataset.historyMode;
  histLimit = HIST_PAGE;
  $$("[data-history-mode]").forEach(option => option.setAttribute("aria-pressed", String(option === button)));
  renderHistory();
});
$(".main").addEventListener("scroll", () => {
  if (page !== "history" || historyScrollFrame) return;
  historyScrollFrame = requestAnimationFrame(() => {
    historyScrollFrame = 0;
    if (page !== "history") return;
    const main = $(".main"), groups = $$("#hist-list .history-group");
    if (!groups.length || main.scrollTop === historyJumpTop) return;
    historyJumpTop = null;
    if (main.scrollTop > 0 && main.scrollHeight - main.clientHeight - main.scrollTop <= 2) {
      selectHistoryGroup(groups.length - 1);
      return;
    }
    const boundary = main.getBoundingClientRect().top + 48;
    let index = 0;
    for (const [i, group] of groups.entries()) {
      if (group.getBoundingClientRect().top > boundary) break;
      index = i;
    }
    selectHistoryGroup(index);
  });
}, {passive:true});
const historyDigits = Array.from((9876543210).toLocaleString(undefined, {useGrouping:false})).reverse();
function historyMetric(value, animate) {
  const exact = esc(value);
  if (!animate) return exact;
  let rolling = false;
  const strips = Array.from(String(value), character => {
    const digit = historyDigits.indexOf(character);
    if (digit < 0) return esc(character);
    rolling = true;
    const digits = Array.from({length:5}, (_, i) => `<span>${esc(historyDigits[(digit + i + 6) % 10])}</span>`).join("");
    return `<span class="metric-reel"><span class="metric-strip">${digits}</span></span>`;
  }).join("");
  return rolling ? `<span class="sr-only">${exact}</span><span aria-hidden="true">${strips}</span>` : exact;
}
let historyRequest = 0, historyEntryPending = false, historyStatsKey;
function historyTrend(key, series = []) {
  const today = key === "today", seconds = key === "latency";
  const unit = seconds ? " s" : key === "wpm" ? " wpm" : "";
  const places = seconds ? 1 : 0;
  const display = value => value.toLocaleString(undefined, {maximumFractionDigits:places, minimumFractionDigits:places});
  const current = series.at(-1), previous = series.at(-2);
  const observed = series.filter(Number.isFinite);
  let comparison = "No earlier comparison";
  if (Number.isFinite(current) && Number.isFinite(previous)) {
    const delta = +(Number(current.toFixed(places)) - Number(previous.toFixed(places))).toFixed(places);
    const direction = delta > 0 ? "↑ +" : delta < 0 ? "↓ −" : "";
    const change = delta ? direction + display(Math.abs(delta)) + unit : "No change";
    comparison = `<b>${change}</b> <span>${today ? "vs same time yesterday" : "vs previous 7 days"}</span>`;
  }
  let graph = '<p class="metric-range">Not enough history for a trend</p>';
  if (observed.length >= 2 && Number.isFinite(current)) {
    const upper = Math.max(...observed);
    const bars = series.map(value => Number.isFinite(value)
      ? `<i style="--metric-bar:${upper ? 100 * value / upper : 0}%"></i>`
      : '<i class="metric-gap"></i>').join("");
    graph = `<div class="metric-bars" aria-hidden="true">${bars}</div>
      <p class="metric-range"><span>${today ? "Last 7 days" : "Last 4 weeks"}</span><b>${display(observed[0])} → ${display(current)}${unit}</b></p>`;
  }
  return `<div class="metric-tip" id="metric-tip-${key}" role="tooltip"><p class="metric-delta">${comparison}</p>${graph}</div>`;
}

async function loadHistory(entering = false) {
  historyEntryPending ||= entering;
  const request = ++historyRequest;
  const history = await api().get_history(true);
  if (request !== historyRequest) return;
  histAll = history;
  renderHistory();
  const st = await api().get_stats();
  if (request !== historyRequest || page !== "history") return;
  const key = JSON.stringify(st);
  if (!historyEntryPending && key === historyStatsKey) return;
  const animate = historyEntryPending && !window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  historyEntryPending = false;
  historyStatsKey = key;
  const stat = (key, value, unit, label, detail = "") =>
    `<div class="stat" id="history-metric-${key}" tabindex="0" aria-describedby="metric-tip-${key}"><div class="l">${label}</div><div class="v"><span class="metric-digits">${historyMetric(value, animate)}</span>${unit ? `<small>${unit}</small>` : ""}</div><div class="s">${detail}</div>${historyTrend(key, st.trends?.[key])}</div>`;
  const stats = $("#stats");
  stats.innerHTML = stat("today", st.today, "today", "Dictations", `${st.week} this week`) +
    stat("words_week", st.words_week.toLocaleString(), "words", "This week", `${st.minutes_week.toFixed(1)} min spoken`) +
    stat("wpm", st.wpm || "–", "wpm", "Speaking speed") +
    stat("latency", st.latency ? st.latency.toFixed(1) : "–", st.latency ? "s" : "", "Processing", "Speech and formatting") +
    `<p class="stats-meta">$${st.spend_month.toFixed(2)} est. API this month${st.spend_unknown ? ` · ${st.spend_unknown} unpriced calls` : ""}${st.spend_note ? " · " + esc(st.spend_note) : ""}</p>`;
  if (animate) for (const strip of stats.querySelectorAll(".metric-strip")) {
    const reel = strip.animate([{transform:"none"}, {transform:"translateY(-80%)"}],
      {duration:1500,easing:"cubic-bezier(.2,.5,.4,1)",fill:"both"});
    reel.onfinish = () => strip.parentElement.replaceWith(strip.lastElementChild.textContent);
  }
}
$("#hist-search").oninput = () => { clearTimeout(histTimer); histTimer = setTimeout(() => { histLimit = HIST_PAGE; renderHistory(); }, 200); };
async function clearPrivate(scope) {
  const meta = {
    history: { title: "Clear dictation history?", desc: "dictation history", btn: "Clear history" },
    workspace: { title: "Clear drafts and instructions?", desc: "current and archived recovery drafts, instructions and pending suggestions", btn: "Clear drafts" },
    learned: { title: "Clear learned preferences?", desc: "learned preferences", btn: "Clear" },
    logs: { title: "Clear app logs?", desc: "application logs (last-error.txt and last-crash.txt)", btn: "Clear logs" },
  }[scope] || { title: "Please confirm", desc: scope, btn: "Clear" };
  if (!await askConfirm(`Clear ${meta.desc}? Favorites and cost accounting remain. Older recovery backups may remain. This cannot be undone.`, { title: meta.title, confirmLabel: meta.btn, destructive: true })) return;
  try { const result=await api().clear_private_data(scope,"DELETE"); if(scope==="history"||scope==="workspace")invalidatePreview(result.preview_version); toast(result.message || (result.ok ? "Cleared" : "Could not clear"),!result.ok); if(result.ok) { if(scope==="history") { loadHistory();loadRecent(); } if(scope==="workspace") { labClearVersion++;feedbackRequest=null;feedbackIssues.clear();editorDirty=false;composerDirty=false;await loadLab(); } } } catch { toast("Clear was not confirmed. Reconnect and inspect before trying again.",true); }
}
$("#btn-clear-history").onclick = () => clearPrivate("history");
$$("[data-clear]").forEach(b=>b.onclick=()=>clearPrivate(b.dataset.clear));
async function loadRecent() {
  const history = await api().get_history();
  const rows = history.slice(0, 8);
  recentShown = rows;
  $("#recent-list").innerHTML = rows.length ? rows.map(histRow).join("") : '<li class="empty">Your dictations will show up here.</li>';
  bindRows($("#recent-list"), recentShown);
}

// ---------- prompt lab ----------
const ed = $("#draft"), composer = $("#composer");
ed.addEventListener("focus", () => editing = true);
ed.addEventListener("blur", () => { editing = false; flushEdit(); });
let composerTimer, composerVersion=0, composerDirty=false;
let editTimer, editVersion = 0, labRequest = 0, editorContext, editorDirty = false;
let publishingDraft = false;
ed.addEventListener("input", () => {
  editing = true; editorDirty = true; editVersion++; clearTimeout(editTimer);
  renderLabActivity();
  const text = ed.value, context = editorContext, version = editVersion;
  editTimer = setTimeout(() => saveEdit(text, context, version), 300);
});
const KIND = { typed: "typed", engineer: "engineered", refine: "refined", improve: "improved", rebuild: "rebuilt", favorite: "favorite" };

async function loadLab(checkOnly = false) {
  if (publishingDraft) return false;
  const request = ++labRequest, version = editVersion, instructionVersion = composerVersion;
  const superseded = () => request !== labRequest || version !== editVersion || instructionVersion !== composerVersion;
  let next = await api().get_lab();
  if (superseded() || publishingDraft) return false;
  if (!checkOnly && !next.busy && next.prepared_pass && !editorDirty && !composerDirty && !ed.readOnly && !composer.readOnly
      && next.draft_id === editorContext && draftMatches(next.text)) {
    publishingDraft = ed.readOnly = composer.readOnly = true;
    try { next = await api().finish_draft_pass(next.draft_id, next.prepared_pass); }
    finally { publishingDraft = ed.readOnly = composer.readOnly = false; }
    if (superseded()) return false;
  }
  if (next.draft_id !== editorContext && (editorDirty || composerDirty)) {
    if (!api().retain_editor) return false;
    if (editorDirty && !await api().retain_editor(ed.value)) return false;
    if (composerDirty && !await api().retain_editor("Unsent instruction:\n" + composer.value)) return false;
    if (superseded()) return false;
    editorDirty = false; composerDirty = false;
    recovery("Unsaved work was kept in recovery before switching drafts.");
  }
  lab = next;
  editVersion = Math.max(editVersion, Number(next.edit_id) || 0);
  composerVersion = Math.max(composerVersion, Number(next.composer_edit_id) || 0);
  if (!editorDirty || next.draft_id !== editorContext) {
    setDisplayedProperty(ed, "value", lab.text || "");
    if (next.draft_id !== editorContext) ed.scrollTop = 0;
    editorContext = lab.draft_id;
  }
  if (!composerDirty) setDisplayedProperty(composer, "value", lab.composer || "");
  recovery(editorDirty || composerDirty ? "Unsaved changes" : lab.save_error ? "Recovery could not be saved: " + lab.save_error : lab.saved ? "Saved locally" : "Recovery not yet saved");
  renderCandidate(); loadArchives();
  ed.classList.toggle("busy", !!lab.busy);
  // meta line
  const estimate = lab.session_cost ? `Estimated session $${lab.session_cost.toFixed(3)}` : "";
  const unpriced = lab.session_unpriced ? `${lab.session_unpriced} unpriced API attempt${lab.session_unpriced === 1 ? "" : "s"}` : "";
  setDisplayedProperty($("#lab-cost"), "textContent", [estimate, unpriced].filter(Boolean).join(" · "));
  $("#draft-version").textContent = lab.cur >= 0 ? `v${lab.cur + 1}` : "";
  $("#btn-revisions").classList.toggle("hidden", !lab.revs.length);
  $("#draft-meta").innerHTML = lab.revs.length
    ? `v${lab.cur + 1} · ${esc(KIND[lab.revs[lab.cur]?.kind] || "")}${lab.by ? " by " + esc(lab.by) : ""}${lab.fav ? ` · favorite “${esc(lab.fav)}”` : ""}`
    : "Paste a prompt, type one, or dictate one here.";
  // revisions strip
  stableHTML($("#revs"), lab.revs.map(r => `<button class="rev ${r.i === lab.cur ? "cur" : ""}" data-i="${Number(r.i)}"${r.i === lab.cur ? ' aria-current="true"' : ""} title="${esc(KIND[r.kind] || r.kind)}${r.by ? " by " + esc(r.by) : ""}">v${Number(r.i) + 1}${r.score != null ? ` <b ${scoreAttributes(r.score)}>${Number(r.score)}</b>` : ""}${r.stale ? ' <span class="stale">edited</span>' : ""}</button>`).join(""));
  $$("#revs .rev").forEach(b => b.onclick = async () => {
    if (!await flushAll()) return;
    editVersion++;
    await api().select_rev(+b.dataset.i);
    editing = false;
    await loadLab();
    const panel = $("#lab-revisions");
    if (panel.matches(":popover-open")) {
      const restoreFocus = panel.contains(document.activeElement) || document.activeElement === document.body;
      panel.hidePopover();
      if (restoreFocus) $("#btn-revisions").focus();
    }
  });
  // score bar
  const sb = $("#scorebar"), sc = lab.score;
  if (sc) {
    sb.classList.remove("hidden");
    const p = $("#score-prev");
    if (lab.prev_score == null) { stableHTML(p, ""); p.className = "score-prev muted"; }
    else {
      const previous = lab.prev_score, delta = sc.score - previous, version = prevIdx() + 1;
      stableHTML(p, delta === 0 ? `same as v${version}`
        : `${delta > 0 ? "▲" : "▼"} ${Math.abs(delta)} vs v${version} (<b ${scoreAttributes(previous)}>${Number(previous)}</b>)`);
      p.className = "score-prev " + (delta > 0 ? "up" : delta < 0 ? "down" : "muted");
    }
    $("#score-gaps").textContent = `${sc.gaps.length} gap${sc.gaps.length === 1 ? "" : "s"} · ${sc.by}${sc.cost ? ` · $${sc.cost.toFixed(3)}` : ""}${sc.note ? " · " + sc.note : ""}`;
  } else sb.classList.add("hidden");
  // buttons
  const has = !!(lab.text || "").trim(), busy = !!lab.busy;
  const completed = feedbackRequest?.id && !busy && !next.prepared_pass ? feedbackRequest : null;
  const displayedResult = completed && completed.workspace === lab.workspace_id && labResultId(completed.action) === completed.id;
  if (completed) {
    feedbackRequest = null;
    if (!displayedResult && (completed.context !== editorContext || completed.edit !== editVersion)) {
      const reason = completed.context === editorContext ? "you edited the draft" : "you switched drafts or revisions";
      const work = completed.action === "feedback" ? "Feedback" : `${completed.action} result`;
      if (!feedbackIssues.get(lab.workspace_id)?.uncertain) feedbackIssues.set(lab.workspace_id, {message:`${work} for ${completed.label} is unavailable after ${reason}.`});
    }
  }
  $("#btn-improve").disabled = !has || busy; $("#btn-rebuild").disabled = !has || busy;
  composer.disabled = false;
  composer.placeholder = has ? "Describe what you want to change." : "Describe the prompt you want to write.";
  const fieldError = lab.error_op ? "" : lab.error || "";
  setDisplayedProperty($("#lab-action-error"), "textContent", fieldError); $("#lab-action-error").classList.toggle("hidden", !fieldError);
  if (fieldError && fieldError !== lastLabErr && !reviewDialog.open) toast(fieldError, !/already as strong/.test(fieldError));
  lastLabErr = lab.error;
  renderLabActivity();
  if (displayedResult && completed.action === "Engineer" && !completed.leftLab && page === "lab" && document.activeElement === $("#btn-draft-pass")) {
    $("#btn-copy").focus({preventScroll:true});
  }
  if (displayedResult && completed.action === "feedback" && currentFeedbackIntent(completed)
      && document.activeElement === $("#btn-score")
      && !$("dialog[open], :popover-open") && moreMenu.classList.contains("hidden")) {
    openReview(lab.pending && !lab.pending.stale ? lab.pending : null, $("#btn-score"));
  }
  return checkOnly || !next.prepared_pass;
}
let lastLabErr = "";
const SCORE_LABELS = {strong:"Strong", fair:"Fair", "needs-work":"Needs work", none:""};
function scoreQuality(value, scale = 10) {
  if (!Number.isFinite(value) || value < 0 || value > scale) return "none";
  return value >= scale * .8 ? "strong" : value >= scale * .6 ? "fair" : "needs-work";
}
function scoreAttributes(value, scale = 10) {
  const quality = scoreQuality(value, scale);
  return `data-score-quality="${quality}" title="${SCORE_LABELS[quality]}"`;
}
function setScoreBadge(badge, value, label) {
  const quality = scoreQuality(value);
  setDisplayedProperty(badge.dataset, "scoreQuality", quality);
  setDisplayedProperty(badge, "title", SCORE_LABELS[quality]);
  setDisplayedProperty(badge, "textContent", label);
}

function prevIdx() { for (let i = lab.cur - 1; i >= 0; i--) if (lab.revs[i].score != null) return i; return -1; }
async function saveEdit(text, context, version) {
  recovery("Saving locally…");
  try { const saved=await api().set_text(text,context,version); if(version===editVersion && context===editorContext) { if(saved) editorDirty=false; recovery(saved ? "Saved locally" : "Draft not saved. Your text remains here; try again before continuing."); } return saved; }
  catch { if (version === editVersion && context === editorContext) recovery("Save not confirmed. Your text remains here; reconnect before continuing."); return false; }
}
async function flushEdit() { clearTimeout(editTimer); return !editorDirty || await saveEdit(ed.value,editorContext,editVersion); }
async function saveComposer(text,context,version) {
  recovery("Saving instruction…");
  try { const ok=await api().set_composer(text,context,version); if(version===composerVersion && context===editorContext) { if(ok) composerDirty=false; recovery(ok ? "Saved locally" : "Instruction not saved. Your input remains here."); } return ok; }
  catch { if (version === composerVersion && context === editorContext) recovery("Instruction save not confirmed. Your input remains here."); return false; }
}
composer.addEventListener("input",()=> { composerDirty=true;composerVersion++;clearTimeout(composerTimer);const text=composer.value,context=editorContext,version=composerVersion; recovery("Unsaved instruction");composerTimer=setTimeout(()=>saveComposer(text,context,version),350); });
async function flushAll() { clearTimeout(composerTimer); if(!await flushEdit()) return false; return !composerDirty || await saveComposer(composer.value,editorContext,composerVersion); }
let quitPending = false;
window.requestDictationQuit = async () => {
  if (quitPending) return;
  if (publishingDraft) { toast("Your draft is still being saved. Wait for it to finish, then close Listen.", true); return; }
  if (vocabAdding) { toast("Vocabulary is still being updated. Wait for it to finish, then choose Quit again.", true); return; }
  quitPending = true;
  const fields = [ed, composer, vocab], readOnly = fields.map(el => el.readOnly);
  const context = editorContext, versions = [editVersion, composerVersion, vocabVersion], text = fields.map(el => el.value);
  fields.forEach(el => el.readOnly = true);
  let accepted = false;
  try {
    if (!await flushAll() || !await flushVocab()) { toast("Changes could not be saved. Keep the app open and try again.", true); return; }
    if (context !== editorContext || versions.some((v, i) => v !== [editVersion, composerVersion, vocabVersion][i]) || fields.some((el, i) => el.value !== text[i]) || editorDirty || composerDirty || vocabDirty) {
      toast("Your work changed while saving. Review it, then choose Quit again.", true); return;
    }
    const result = await api().quit_app();
    accepted = result === true;
    if (!accepted) {
      toast(typeof result === "string" ? result : "Listen could not close. Try again.", true);
    }
  } catch (e) { toast("The app could not close. Try Quit again.", true); }
  finally { if (!accepted) { fields.forEach((el, i) => el.readOnly = readOnly[i]); quitPending = false; } }
};
$("#btn-quit").onclick = window.requestDictationQuit;
function positionLabPopover(panel) {
  const trigger = $(`[popovertarget="${panel.id}"]`).getBoundingClientRect();
  panel.style.left = Math.max(8, Math.min(trigger.left, innerWidth - panel.offsetWidth - 8)) + "px";
  panel.style.top = Math.max(8, Math.min(trigger.bottom + 6, innerHeight - panel.offsetHeight - 8)) + "px";
}
for (const panel of $$(".lab-popover")) {
  panel.addEventListener("toggle", ev => {
    if (ev.newState === "open") positionLabPopover(panel);
  });
}
// "More actions" menu: opens above its button, closes on outside click, on choosing an item, or on Escape (focus returns to the button).
const moreBtn = $("#btn-more"), moreMenu = $("#more-menu");
function closeMore(focus) {
  if (moreMenu.classList.contains("hidden")) return false;
  moreMenu.classList.add("hidden"); moreBtn.setAttribute("aria-expanded", "false"); if (focus) moreBtn.focus();
  return true;
}
function positionMore() {
  const r = moreBtn.getBoundingClientRect();
  moreMenu.style.left = Math.max(8, Math.min(r.left, innerWidth - moreMenu.offsetWidth - 8)) + "px";
  moreMenu.style.bottom = (innerHeight - r.top + 6) + "px";
  moreMenu.style.maxHeight = Math.max(40, r.top - 14) + "px";
}
moreBtn.onclick = () => {
  if (closeMore(false)) return;
  moreMenu.classList.remove("hidden"); moreBtn.setAttribute("aria-expanded", "true");
  positionMore();
};
window.addEventListener("resize", () => {
  for (const panel of $$(".lab-popover:popover-open")) positionLabPopover(panel);
  if (!moreMenu.classList.contains("hidden")) positionMore();
});
moreMenu.addEventListener("click", ev => {
  const button = ev.target.closest("button");
  if (button && button.id !== "btn-delete-draft") closeMore(true);
});
document.addEventListener("click", ev => { if (!ev.target.closest(".more-actions, dialog")) closeMore(false); });
for(const [id,method] of [["improve","improve"],["rebuild","rebuild"]]) $("#btn-"+id).onclick=async()=> {
  if(suggestionWaiting())return;
  if(!await flushAll())return;
  if(suggestionWaiting())return;
  try { if(!await api()[method]()) toast("Action not started. Check processing choice or wait for current work.",true); }
  catch { toast("Result not confirmed. Check the draft before explicitly trying again.",true); }
};
function currentFeedbackIntent(request) {
  return !request.leftLab && page === "lab" && request.context === editorContext
    && request.edit === editVersion && request.instruction === composerVersion;
}
function feedbackDraftLabel() {
  const title = ed.value.trim().split("\n", 1)[0];
  return title ? `“${title.length > 44 ? title.slice(0, 43) + "…" : title}”` : "the current draft";
}
function rememberUnconfirmedRequest(request, work) {
  if (request.clearVersion !== labClearVersion) return;
  feedbackIssues.set(request.workspace, {...request, uncertain:true,
    message:`${work} was not confirmed for ${request.label}. Check for a result before trying again.`});
  const workspace = lab.workspace_id;
  if (workspace !== request.workspace && !feedbackIssues.get(workspace)?.uncertain) {
    feedbackIssues.set(workspace, {switched:true, attention:true,
      message:`The request for ${request.label} was not confirmed after you switched drafts.`});
  }
}
async function requestLab(action) {
  const workspace = lab.workspace_id, issue = feedbackIssues.get(workspace);
  if (feedbackRequest || sending || lab.busy || lab.prepared_pass || !ed.value.trim() || issue?.checking || (issue?.uncertain && !issue.checked)) return;
  if (action !== "feedback" && lab.pending) {
    if (!issue?.uncertain) feedbackIssues.set(workspace, {waiting:true, message:"Open Compare suggestion to review or discard the earlier suggestion before continuing."});
    renderLabActivity();
    return;
  }
  const request = {action, workspace, context: editorContext, revision: Math.max(0, lab.cur), edit: editVersion, instruction: composerVersion, clearVersion: labClearVersion,
    label: feedbackDraftLabel(), previous: labResultId(action)};
  const work = action === "feedback" ? "Feedback" : action;
  feedbackRequest = request;
  renderLabActivity();
  try {
    if (!await flushAll() || !currentFeedbackIntent(request)) return;
    try {
      request.id = action === "feedback" ? await api().feedback(request.context, ed.value)
        : await api().draft_pass(request.context, ed.value, action === "Refine");
    } catch {
      rememberUnconfirmedRequest(request, work);
      return;
    }
    if (!request.id) {
      if (!issue?.uncertain) feedbackIssues.set(workspace, {message:`${work} could not start for ${request.label}. Check your draft and try again.`, attention:true});
      return;
    }
    feedbackIssues.delete(workspace);
    await loadLab().catch(() => false); // Polling retries a failed refresh.
  } finally {
    if (!request.id && feedbackRequest === request) {
      feedbackRequest = null;
      renderLabActivity();
    }
  }
}
$("#btn-score").onclick = () => requestLab("feedback");
$("#btn-draft-pass").onclick = () => requestLab(draftGrade() ? "Refine" : "Engineer");
$("#btn-check-feedback").onclick = async () => {
  const workspace = lab.workspace_id, issue = feedbackIssues.get(workspace);
  if (!issue?.uncertain || issue.checking || feedbackRequest || lab.busy || sending) return;
  issue.checking = true;
  renderLabActivity();
  try {
    if (!await loadLab(true) || lab.workspace_id !== workspace || lab.busy || lab.prepared_pass || feedbackIssues.get(workspace) !== issue) return;
    const pass = issue.action === "Engineer" || issue.action === "Refine";
    const result = labResultId(issue.action);
    const found = result && result !== issue.previous && (issue.action === "Write suggestion" ? !lab.pending.stale
      : draftGrade() && (!pass || lab.draft_pass?.source_id === issue.context));
    if (Math.max(0, lab.cur) !== issue.revision && !(pass && found)) {
      issue.message = `Open v${issue.revision + 1} of ${issue.label} to check the result.`;
      return;
    }
    issue.checked = true;
    const work = issue.action === "feedback" ? "feedback" : "result";
    issue.message = found ? "" : `No new ${work} found for ${issue.label}. You can check again or start a new request for the current draft.`;
  } catch {
    issue.checked = false;
    issue.message = `The request for ${issue.label} is still unconfirmed. Reconnect and check again.`;
  } finally {
    issue.checking = false;
    renderLabActivity();
  }
};
let sending=false;
function draftGrade() {
  return !lab.stale && draftMatches(lab.text) ? lab.score : null;
}
function draftMatches(text) {
  return ed.value === (text || "").replace(/\r\n?/g, "\n");
}
function labResultId(action) {
  if (action === "feedback") return lab.score?.feedback_id;
  if (action === "Write suggestion") return lab.pending?.id;
  return lab.draft_pass?.draft_id === editorContext ? lab.draft_pass?.id : undefined;
}
function suggestionWaiting() {
  if(!lab.pending)return false;
  setDisplayedProperty($("#lab-action-error"), "textContent", "Open Compare suggestion to review it, then apply or discard it before requesting another.");
  $("#lab-action-error").classList.remove("hidden");
  renderLabActivity();
  return true;
}
function renderLabActivity() {
  const issue = feedbackIssues.get(lab.workspace_id), check = $("#btn-check-feedback");
  const notice = !issue?.waiting || lab.pending ? issue?.message : "";
  const grade = draftGrade(), ready = !!grade?.feedback_id;
  const locked = !!lab.busy || !!lab.prepared_pass || sending || !!feedbackRequest;
  const action = feedbackRequest?.action;
  const passMessage = grade && lab.draft_pass?.draft_id === editorContext ? lab.draft_pass.message : "";
  const message = action === "Engineer" ? `Engineering and scoring ${feedbackRequest.label}…`
    : action === "Refine" ? `Refining and scoring ${feedbackRequest.label}…`
    : feedbackRequest ? `Getting feedback for ${feedbackRequest.label}…`
    : sending ? "Writing suggestion…"
    : issue?.switched ? notice
    : lab.busy === "scoring" ? "Getting feedback…"
    : lab.busy === "reviewing" ? "Reviewing suggestion…"
    : lab.busy === "refining" ? "Refining and scoring…"
    : lab.busy ? "Writing suggestion…"
    : issue?.checking ? `Checking for a result for ${issue.label}…`
    : notice || (lab.error_op ? lab.error : "")
      || passMessage || (ready ? `Feedback ready for ${feedbackDraftLabel()}.` : lab.pending ? "Suggestion ready" : "");
  const status = $("#lab-busy");
  setDisplayedProperty(status, "textContent", message);
  status.dataset.ready = String(message === "Suggestion ready" || message.startsWith("Feedback ready"));
  status.dataset.attention = String(!locked && !issue?.checking && !!(issue?.attention || (issue?.uncertain && !issue.checked) || (lab.error_op && lab.error)));
  setDisplayedProperty($("#btn-score"), "disabled", !ed.value.trim());
  const blocked = locked || !!issue?.checking || !!(issue?.uncertain && !issue.checked);
  setDisplayedProperty($("#btn-score"), "ariaDisabled", String(blocked));
  const pass = $("#btn-draft-pass"), badge = $("#score-badge"), copy = $("#btn-copy");
  setDisplayedProperty(pass, "ariaDisabled", String(blocked || !ed.value.trim()));
  setDisplayedProperty(pass, "textContent", grade ? "Refine" : "Engineer");
  pass.classList.toggle("primary", !grade);
  setScoreBadge(badge, grade?.score, grade ? `Score ${grade.score}/10` : "Not scored");
  copy.classList.toggle("hidden", !grade);
  setDisplayedProperty(copy, "ariaDisabled", String(locked));
  setDisplayedProperty($("#btn-send"), "ariaDisabled", String(blocked));
  check.classList.toggle("hidden", !issue?.uncertain);
  setDisplayedProperty(check, "ariaDisabled", String(locked || !!issue?.checking));
  $("#btn-review").classList.toggle("hidden", !lab.pending && !ready);
  setDisplayedProperty($("#review-action-label"), "textContent", lab.pending ? "Compare suggestion" : "View feedback");
}
async function send() {
  const workspace = lab.workspace_id, issue = feedbackIssues.get(workspace);
  if (sending || lab.busy || lab.prepared_pass || feedbackRequest || issue?.checking || (issue?.uncertain && !issue.checked)) return;
  if(suggestionWaiting())return;
  const text=composer.value.trim(), version=composerVersion, context=editorContext;
  if(!text) { $("#lab-action-error").textContent="Describe what you want in Instruction, then choose Write suggestion."; $("#lab-action-error").classList.remove("hidden"); composer.focus(); return; }
  sending=true;renderLabActivity();
  try {
    if(!await flushAll())return;
    if(suggestionWaiting())return;
    if(context!==editorContext || version!==composerVersion || text!==composer.value.trim()) {
      recovery("The draft or instruction changed. Nothing was sent; review your current work before sending.");
      return;
    }
    const action = "Write suggestion";
    const request = {action, workspace, context, revision: Math.max(0, lab.cur), clearVersion: labClearVersion, label: feedbackDraftLabel(), previous: labResultId(action)};
    try {
      if (!await api().submit(text,context)) { if (context === editorContext) recovery("Instruction was not sent. Your input remains here."); return; }
    } catch {
      rememberUnconfirmedRequest(request, "Suggestion request");
      return;
    }
    feedbackIssues.delete(workspace);
    if (context === editorContext) recovery("Instruction accepted. It stays recoverable until a suggestion is ready.");
    await loadLab().catch(() => false); // Polling retries a failed refresh after submission.
  } finally {sending=false;renderLabActivity();}
}
$("#btn-send").onclick=send;
composer.addEventListener("keydown",ev=> { if(ev.key==="Enter"&&!ev.shiftKey&&!ev.ctrlKey&&!ev.altKey&&!ev.metaKey&&!ev.isComposing&&ev.keyCode!==229) {ev.preventDefault();send();} });
$("#btn-copy").onclick=async()=> {
  const text = ed.value, context = editorContext;
  if (!text.trim() || feedbackRequest || lab.busy || sending || !await flushAll() || context !== editorContext || ed.value !== text) return;
  try {
    if (await api().copy_text(text)) { api().lab_kept(); toast("Copied draft."); }
    else toast("Copy failed",true);
  } catch { toast("Copy not confirmed",true); }
};
$("#btn-paste").onclick=async()=> {if(!ed.value.trim()||!await flushAll())return;try{if(!await api().paste_draft(ed.value))toast("No verified destination is ready. Choose Copy and paste where you want the draft.",true);}catch{toast("Paste not confirmed. Check your destination before trying again.",true);} };
$("#btn-new").onclick=async()=> {if(!await flushAll())return;try{if(await api().new_draft()){editVersion++;await loadLab();ed.focus();}else toast("Could not create a draft; your text remains.",true);}catch{toast("New draft not confirmed; your text remains.",true);} };
function askFavoriteName() {const dialog=$("#favorite-name-dialog"),input=$("#favorite-name"),opener=document.activeElement;if(dialog.open)return Promise.resolve(null);input.value=lab.fav||"";dialog.returnValue="";return new Promise(resolve=>{dialog.addEventListener("close",()=>{opener?.focus();resolve(dialog.returnValue==="save"?input.value.trim():null);},{once:true});dialog.showModal();input.focus();});}
$("#btn-save").onclick=async()=> {if(!ed.value.trim()||!await flushAll())return;const name=await askFavoriteName();if(!name)return;try {let result=await api().save_favorite_checked(name,ed.value,false);if(result.status==="conflict"&&await askConfirm(`Replace favorite “${name}”? Its existing text will be replaced. The previous version remains in archived recovery drafts.`))result=await api().save_favorite_checked(name,ed.value,true);if(result.status==="saved"){toast("Favorite saved");loadFavs();}else if(result.status==="error")toast("Favorite could not be saved",true);}catch{toast("Favorite save not confirmed. Inspect Library before trying again.",true);} };
const reviewDialog = $("#candidate"), reviewButton = $("#btn-review");
let reviewId, reviewDraftId, reviewOpener, reviewVersion, reviewCount, showDraftFeedback, reviewingVersion;
let resolvingCandidate = false, startingReview = false, candidateError = "";
function renderCandidate() {
  if (!reviewDialog.open) return;
  const pending = reviewId ? lab.pending : null;
  if (reviewDraftId !== editorContext || (reviewId && pending?.id !== reviewId)) { reviewDialog.close(); return; }
  const versions = pending?.versions || (pending ? [pending] : []);
  const reviewing = startingReview || lab.busy === "reviewing";
  if (!reviewing && versions.length > reviewCount) {
    reviewCount = versions.length;
    reviewVersion = reviewCount - 1;
    showDraftFeedback = false;
    $("#review-show-suggestion").checked = true;
  }
  const selected = versions[reviewVersion];
  const stale = pending && (pending.stale || pending.source_text !== ed.value || editorDirty);
  const actionLocked = !!lab.busy || startingReview || resolvingCandidate;
  const draftLabel = `Draft v${lab.cur + 1}`;
  const suggestionLabel = `Suggestion ${reviewVersion + 1}`;
  reviewDialog.toggleAttribute("data-feedback-only", !selected);
  setDisplayedProperty($("#candidate-title"), "textContent", selected ? "Compare suggestion" : "Draft feedback");
  setDisplayedProperty($("#candidate-original"), "value", pending?.source_text ?? lab.text);
  setDisplayedProperty($("#candidate-text"), "value", selected?.text ?? "");
  setDisplayedProperty($("#candidate-label"), "textContent", suggestionLabel);
  setDisplayedProperty($("#candidate-version"), "textContent", stale ? "Earlier draft" : draftLabel);
  setDisplayedProperty($("#candidate-model"), "textContent", selected?.by || "");
  $(".candidate-grid").classList.toggle("hidden", !selected && lab.stale);
  $("#btn-apply").disabled = !selected || !!stale || actionLocked;
  $("#btn-review-suggestion").disabled = !selected || !!stale || actionLocked;
  $("#btn-discard").disabled = resolvingCandidate || reviewing;
  const note = stale ? "This suggestion is for an earlier draft. Discard it before requesting a new suggestion for your current draft."
    : !selected && lab.pending?.stale ? "An earlier suggestion is still available under Compare suggestion."
    : !selected && !lab.stale ? "This feedback came without a new suggestion." : "";
  $("#candidate-note").classList.toggle("hidden", !note);
  setDisplayedProperty($("#candidate-note"), "textContent", note);

  const rewrite = selected?.score?.improved.trim();
  setDisplayedProperty($("#candidate-status"), "textContent", reviewing
    ? (reviewingVersion === undefined ? "Reviewing suggestion…" : `Reviewing Suggestion ${reviewingVersion + 1}…`)
    : selected && !stale && !selected.score ? `${suggestionLabel} is awaiting review.`
    : selected?.score && !stale && (!rewrite || rewrite === selected.text.trim()) ? "Review complete. No new suggestion." : "");
  renderReviewFeedback(versions, selected, draftLabel, stale || (pending && lab.stale), actionLocked);
  const error = $("#candidate-error");
  const message = candidateError || (reviewId && lab.error_op === "reviewing" && !reviewing ? lab.error : "") || "";
  setDisplayedProperty(error, "textContent", message);
  error.classList.toggle("hidden", !message);
}
function renderReviewFeedback(versions, selected, draftLabel, hideDraftScore, actionLocked) {
  const draftScore = hideDraftScore ? null : lab.score;
  const round = !showDraftFeedback && selected ? (selected.score ? reviewVersion : selected.based_on) : null;
  const name = Number.isInteger(round) ? `Suggestion ${round + 1}` : draftLabel + (lab.stale ? " before edits" : "");
  const score = Number.isInteger(round) ? (selected.score || selected.basis_score) : draftScore;
  setDisplayedProperty($("#candidate-feedback-title"), "textContent", score ? "Feedback on " + name : "Feedback");
  const badge = $("#candidate-score");
  badge.classList.toggle("hidden", !score);
  setScoreBadge(badge, score?.score, score ? `${score.score} / 10` : "");
  stableHTML($("#candidate-gaps"), score
    ? score.gaps.map(gap => `<li>${esc(gap)}</li>`).join("") || "<li>No gaps found.</li>"
    : "<li>No feedback on this suggestion yet.</li>");
  $("#candidate-assessment").classList.toggle("hidden", !score);
  if (score) {
    setDisplayedProperty($("#candidate-assessment-meta"), "textContent",
      `${score.by}${score.cost ? ` · $${score.cost.toFixed(3)}` : ""}${score.note ? "\n" + score.note : ""}`);
    stableHTML($("#candidate-criteria"), Object.entries(score.criteria).map(([key, value]) =>
      `<div><span>${esc(key.replaceAll("_", " "))}</span><b ${scoreAttributes(value, 5)}>${Number(value)}/5</b></div>`).join(""));
  }
  const rounds = $("#candidate-rounds"), focus = document.activeElement?.dataset.reviewRound;
  const earlier = lab.revs.filter(rev => !hideDraftScore && rev.i !== lab.cur && rev.score != null);
  $("#candidate-history").classList.toggle("hidden", !versions.length && !earlier.length);
  const button = (index, label, assessment, pressed) =>
    `<button class="btn sm" data-review-round="${index}" aria-pressed="${pressed}"${actionLocked ? " disabled" : ""}>${label} · ${assessment ? `<b ${scoreAttributes(assessment.score)}>${Number(assessment.score)}/10</b>` : "Awaiting review"}</button>`;
  stableHTML(rounds, (draftScore ? button(-1, draftLabel, draftScore, showDraftFeedback || !selected) : "")
    + versions.map((version, i) => button(i, `Suggestion ${i + 1}`, version.score, !showDraftFeedback && i === reviewVersion)).join(""));
  if (focus !== undefined && !rounds.contains(document.activeElement)) {
    rounds.querySelector('[data-review-round="' + Number(focus) + '"]')?.focus({preventScroll: true});
  }
  stableHTML($("#candidate-revision-scores"), earlier.map(rev => `<span>Draft v${rev.i + 1} · <b ${scoreAttributes(rev.score)}>${Number(rev.score)}/10</b>${rev.stale ? " · edited" : ""}</span>`).join(""));
}
function reviewError(message) {
  candidateError = message;
  renderCandidate();
}
function openReview(pending, opener) {
  if ($("dialog[open]") || (!pending && !lab.score)) return;
  if (reviewId !== pending?.id) reviewingVersion = undefined;
  reviewId = pending?.id;
  reviewDraftId = editorContext;
  reviewOpener = opener;
  reviewCount = pending?.versions?.length || (pending ? 1 : 0);
  reviewVersion = Math.max(0, reviewCount - 1);
  showDraftFeedback = false;
  candidateError = "";
  $("#candidate-history").open = false;
  $("#candidate-assessment").open = false;
  $("#review-show-suggestion").checked = true;
  reviewDialog.showModal();
  $(".candidate-body").scrollTop = 0;
  renderCandidate();
}
reviewButton.onclick = () => openReview(lab.pending, reviewButton);
$("#score-details").onclick = () => openReview(lab.pending && !lab.pending.stale ? lab.pending : null, $("#score-details"));
$("#candidate-rounds").onclick = event => {
  const button = event.target.closest("button[data-review-round]");
  if (!button || button.disabled) return;
  const index = Number(button.dataset.reviewRound);
  showDraftFeedback = index === -1;
  if (!showDraftFeedback) reviewVersion = index;
  $("#review-show-original").checked = showDraftFeedback;
  $("#review-show-suggestion").checked = !showDraftFeedback;
  renderCandidate();
};
$("#btn-close-review").onclick = () => reviewDialog.close();
reviewDialog.addEventListener("close", () => {
  if (page !== "lab" || $("dialog[open]") || document.activeElement !== document.body) return;
  const opener = reviewOpener;
  (opener && !opener.disabled && opener.getClientRects().length ? opener : ed).focus();
});
ed.addEventListener("input", renderCandidate);
$("#btn-review-suggestion").onclick = async () => {
  if (!reviewDialog.open || $("#btn-review-suggestion").disabled) return;
  const id = reviewId, version = reviewVersion;
  startingReview = true;
  reviewingVersion = version;
  reviewError("");
  try {
    if (!await api().review_candidate(id, version) && reviewId === id) {
      reviewError("Review could not start. Check the draft and try again.");
    }
    await loadLab().catch(() => false); // Polling retries a failed refresh.
  } catch {
    if (reviewId === id) reviewError("Review was not confirmed. Check the result before trying again.");
  } finally {
    startingReview = false;
    renderCandidate();
  }
};
async function resolveCandidate(apply) {
  if (!reviewDialog.open || $(apply ? "#btn-apply" : "#btn-discard").disabled || lab.pending?.id !== reviewId) return;
  const id = reviewId, version = reviewVersion;
  resolvingCandidate = true;
  reviewError("");
  renderCandidate();
  try {
    if (apply && !await flushAll()) {
      reviewError("Your edits could not be saved. Return to the draft and try again.");
      return;
    }
    const resolved = apply ? await api().apply_candidate(id, version) : await api().discard_candidate(id);
    await loadLab();
    if (!resolved && reviewDialog.open && reviewId === id) {
      reviewError(apply ? "Could not apply this suggestion. Your draft is preserved." : "Could not discard this suggestion. Try again.");
    }
  } catch {
    if (reviewDialog.open && reviewId === id) reviewError("The change could not be confirmed. Return to the draft and check the result.");
  } finally {
    resolvingCandidate = false;
    renderCandidate();
  }
}
$("#btn-apply").onclick = () => resolveCandidate(true);
$("#btn-discard").onclick = () => resolveCandidate(false);
async function loadArchives(){try{const rows=await api().list_drafts(),el=$("#archived-drafts");const html='<option value="">Choose a draft</option>'+rows.map(r=>`<option value="${esc(r.id)}">${esc(r.title)}</option>`).join("");stableHTML(el,html);}catch{/* recovery remains visible */}}
$("#btn-restore").onclick=async()=> {const id=$("#archived-drafts").value;if(!id||!await flushAll())return;try{if(await api().restore_draft(id)){await loadLab();}else toast("Draft could not be restored",true);}catch{toast("Restore not confirmed. Your input remains.",true);} };
$("#btn-delete-draft").onclick=async()=> {const id=$("#archived-drafts").value;if(!id||!await askConfirm("Delete the selected archived draft? Current work and cost accounting remain. Older recovery backups may still contain it."))return;try{if(await api().delete_draft(id,"DELETE")){await loadArchives();toast("Archived draft deleted. Recovery backups may remain.");}else toast("Draft deletion was not saved.",true);}catch{toast("Deletion not confirmed. Inspect the list before retrying.",true);} };
$("#btn-library").onclick = () => { const l = $("#library"); l.classList.toggle("hidden"); if (!l.classList.contains("hidden")) loadFavs(); };
async function loadFavs() {
  favs=await api().get_favorites();
  const previous = document.activeElement, action = previous?.dataset?.name !== undefined ? "name" : previous?.dataset?.remove !== undefined ? "remove" : null;
  $("#fav-list").innerHTML=favs.length ? favs.map(f=>`<li><button class="link favorite-load" data-name="${esc(f.name)}">${esc(f.name)}</button><span class="x muted">${esc(f.text.slice(0,80))}</span><button class="btn sm" data-remove="${esc(f.name)}" aria-label="Remove ${esc(f.name)}">Remove</button></li>`).join("") : '<li class="empty">No favorites yet. Save a draft to reuse it here.</li>';
  if (action) $$("#fav-list [data-" + action + "]").find(b => b.dataset[action] === previous.dataset[action])?.focus();
  $$(".favorite-load").forEach(b=>b.onclick=async()=> {
    if(!await flushAll())return;
    try { if(await api().load_favorite(b.dataset.name)) { await loadLab(); const active = document.activeElement, focus = page === "lab" && $("#fav-list").contains(active) && active.dataset.name === b.dataset.name; $("#library").classList.add("hidden"); if(focus) ed.focus(); } }
    catch {toast("Favorite load not confirmed",true);}
  });
  $$("[data-remove]").forEach(b=>b.onclick=async()=> {
    try {
      const token=await api().remove_favorite(b.dataset.remove); if(!token)return toast("Favorite could not be removed",true);
      const focused=await loadFavs(),undo=document.createElement("button"),undoBox=$("#favorite-undo");
      undo.className="btn";undo.textContent="Undo removal";
      undo.onclick=async()=>{
        try {
          if(await api().undo_favorite_removal(token)) { await loadFavs(); const focus = page === "lab" && document.activeElement === undo; undoBox.replaceChildren(); undoBox.classList.add("hidden"); if(focus) $$("#fav-list .favorite-load").find(el=>el.dataset.name===b.dataset.remove)?.focus(); toast("Favorite restored"); }
          else toast("Could not restore: that name may already exist. Your newer favorite is preserved.",true);
        } catch { toast("Restoration was not confirmed. Try Undo again.",true); }
      };
      undoBox.replaceChildren(undo);undoBox.classList.remove("hidden");if(focused === b && document.activeElement === document.body) undo.focus();toast("Favorite removed. Undo is available.");
    } catch {toast("Removal not confirmed",true);}
  });
  return previous;
}

// ---------- vocabulary ----------
const vocab = $("#vocab"); let vocabTimer, vocabDirty = false, vocabVersion = 0, vocabSave = Promise.resolve(true), vocabAdding = false;
function flushVocab() {
  clearTimeout(vocabTimer);
  const version = vocabVersion, text = vocab.value;
  const save = async () => {
    if (!vocabDirty) return true;
    if (version !== vocabVersion) return false;
    setDisplayedProperty($("#vocab-status"), "textContent", "Saving vocabulary...");
    try {
      const saved = await api().set_setting("vocab", text) === true;
      if (version !== vocabVersion) return false;
      if (saved) { settings.vocab = text; vocabDirty = false; }
      setDisplayedProperty($("#vocab-status"), "textContent", saved ? "Vocabulary saved locally" : "Vocabulary not saved. Your words remain here; keep Listen open and try again.");
      return saved;
    } catch {
      if (version === vocabVersion) setDisplayedProperty($("#vocab-status"), "textContent", "Vocabulary save not confirmed. Your words remain here; keep Listen open.");
      return false;
    }
  };
  vocabSave = vocabSave.then(save, save);
  return vocabSave;
}
vocab.oninput = () => { vocabDirty = true; vocabVersion++; clearTimeout(vocabTimer); setDisplayedProperty($("#vocab-status"), "textContent", "Unsaved vocabulary"); vocabTimer = setTimeout(flushVocab, 400); };
async function addVocabSuggestion(word) {
  if (vocabAdding || quitPending) return;
  vocabAdding = true;
  const readOnly = vocab.readOnly;
  vocab.readOnly = true;
  $$("#vocab-chips [data-add]").forEach(b => b.disabled = true);
  try {
    if (!await flushVocab()) return;
    const version = vocabVersion;
    setDisplayedProperty($("#vocab-status"), "textContent", "Adding vocabulary...");
    if (!await api().add_vocab(word)) { setDisplayedProperty($("#vocab-status"), "textContent", "Vocabulary change was not confirmed. Your words remain here."); return; }
    const latest = await api().get_settings();
    if (version === vocabVersion && !vocabDirty) { settings = latest; vocab.value = latest.vocab; setDisplayedProperty($("#vocab-status"), "textContent", "Vocabulary saved locally"); }
    toast(`Added “${word}”`); await loadSuggestions().catch(() => toast("Vocabulary added. Suggestions could not refresh.", true));
  } catch { setDisplayedProperty($("#vocab-status"), "textContent", "Vocabulary change was not confirmed. Your words remain here; check Settings before trying again."); }
  finally { vocabAdding = false; vocab.readOnly = readOnly; $$("#vocab-chips [data-add]").forEach(b => b.disabled = quitPending); }
}

// ---------- meetings ----------
let meetingState = { active: false, transcribing: false, elapsed_secs: 0, loopback_active: false, loopback_warning: "", status: "", notes: [] };
let meetingsPollTimer = 0, meetingsPollInFlight = false, meetingActionPending = false, lastMeetingsNotesKey = "";
const openMeetingTranscripts = new Set();

function shouldPollMeetings() {
  return page === "meetings" || !!meetingState.active || !!meetingState.transcribing;
}
function stopMeetingsPoll() {
  clearTimeout(meetingsPollTimer);
  meetingsPollTimer = 0;
}
function scheduleMeetingsPoll(immediate = false) {
  stopMeetingsPoll();
  if (!shouldPollMeetings()) return;
  const delay = immediate ? 0 : document.hidden ? 3000 : (meetingState.active || meetingState.transcribing ? 1000 : 2500);
  meetingsPollTimer = setTimeout(() => loadMeetings(false), delay);
}
function fmtMeetingTimer(secs) {
  const total = Math.max(0, Math.floor(Number(secs) || 0));
  const h = Math.floor(total / 3600), m = Math.floor((total % 3600) / 60), s = total % 60;
  const pad = n => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}
function fmtMeetingDuration(secs) {
  const total = Math.max(0, Math.floor(Number(secs) || 0));
  if (total < 60) return `${total}s`;
  const m = Math.floor(total / 60), s = total % 60;
  if (m < 60) return s ? `${m}m ${s}s` : `${m}m`;
  const h = Math.floor(m / 60), rm = m % 60;
  return rm ? `${h}h ${rm}m` : `${h}h`;
}
function fmtMeetingTs(ts) {
  if (!ts) return "";
  const normalized = String(ts).replace(" ", "T");
  const d = new Date(normalized);
  return Number.isFinite(d.getTime()) ? fmtTs(normalized) : String(ts);
}
function formatMeetingSummaryCopy(note) {
  const parts = [];
  if (note.title) parts.push(note.title);
  if (note.summary) parts.push(note.summary);
  if (Array.isArray(note.action_items) && note.action_items.length) {
    parts.push("Action items:\n" + note.action_items.map(item => `- ${item}`).join("\n"));
  }
  return parts.join("\n\n") || note.transcript || "";
}
function renderMeetings(data) {
  if (!data || typeof data !== "object") return;
  const active = !!data.active;
  const transcribing = !!data.transcribing;
  const elapsed = Number(data.elapsed_secs) || 0;
  const loopbackActive = !!data.loopback_active;
  const warning = typeof data.loopback_warning === "string" ? data.loopback_warning.trim() : "";
  const statusText = typeof data.status === "string" ? data.status.trim() : "";
  const notes = Array.isArray(data.notes) ? data.notes : [];
  meetingState = { active, transcribing, elapsed_secs: elapsed, loopback_active: loopbackActive, loopback_warning: warning, status: statusText, notes };

  const navBtn = $('.rail-btn[data-page="meetings"]');
  if (navBtn) navBtn.classList.toggle("is-live", active);

  const card = $("#meeting-recorder");
  if (card) card.classList.toggle("is-active", active);
  const dot = $("#meeting-dot");
  if (dot) dot.className = "dot " + (active ? "recording" : transcribing ? "busy" : "idle");
  const heading = $("#meeting-heading");
  if (heading) setDisplayedProperty(heading, "textContent", active ? "Recording meeting" : transcribing ? "Summarizing meeting…" : "Meeting notes");
  const timer = $("#meeting-timer");
  if (timer) setDisplayedProperty(timer, "textContent", fmtMeetingTimer(elapsed));
  const badge = $("#meeting-source-badge");
  if (badge) {
    const label = active ? (loopbackActive ? "Mic + System audio" : "Mic only") : "Mic + System audio";
    setDisplayedProperty(badge, "textContent", label);
    badge.classList.toggle("active", active);
  }
  const statusEl = $("#meeting-status");
  if (statusEl) {
    const message = transcribing
      ? (statusText || "Transcribing and summarizing locally…")
      : active
        ? (warning || statusText || (loopbackActive ? "Recording mic and system audio locally." : "Recording microphone locally."))
        : (warning || statusText || "Capture your mic and system audio locally, then generate a summary and action items.");
    setDisplayedProperty(statusEl, "textContent", message);
    if (warning && !transcribing) statusEl.dataset.level = "warning";
    else delete statusEl.dataset.level;
  }

  const startBtn = $("#btn-meeting-start"), stopBtn = $("#btn-meeting-stop"), cancelBtn = $("#btn-meeting-cancel");
  if (startBtn && stopBtn && cancelBtn) {
    startBtn.classList.toggle("hidden", active);
    startBtn.disabled = active || transcribing || meetingActionPending;
    stopBtn.classList.toggle("hidden", !active);
    stopBtn.disabled = !active || transcribing || meetingActionPending;
    cancelBtn.classList.toggle("hidden", !active);
    cancelBtn.disabled = !active || transcribing || meetingActionPending;
  }

  const countEl = $("#meetings-count");
  if (countEl) setDisplayedProperty(countEl, "textContent", notes.length ? `${notes.length} ${notes.length === 1 ? "note" : "notes"}` : "");

  const listEl = $("#meetings-list");
  if (!listEl) return;
  const notesKey = JSON.stringify(notes);
  if (notesKey === lastMeetingsNotesKey && listEl.children.length) return;
  lastMeetingsNotesKey = notesKey;

  if (!notes.length) {
    listEl.innerHTML = `<div class="card meeting-empty muted">No meeting notes yet. Start a meeting above to capture and summarize locally.</div>`;
    return;
  }
  listEl.innerHTML = notes.map(note => {
    const id = String(note.id);
    const items = Array.isArray(note.action_items) ? note.action_items.filter(Boolean) : [];
    const hasSummary = !!(note.summary && note.summary.trim());
    const hasTranscript = !!(note.transcript && note.transcript.trim());
    const isOpen = openMeetingTranscripts.has(id);
    return `<article class="card meeting-note" data-meeting-id="${esc(id)}">
      <div class="meeting-note-head">
        <h4 class="meeting-note-title">${esc(note.title || "Meeting notes")}</h4>
        <div class="meeting-note-meta">
          ${note.ts ? `<time datetime="${esc(note.ts)}">${esc(fmtMeetingTs(note.ts))}</time>` : ""}
          <span class="route-tag">${esc(fmtMeetingDuration(note.duration_secs))}</span>
          <span class="route-tag">${note.loopback_used ? "Mic + System" : "Mic only"}</span>
        </div>
      </div>
      ${hasSummary ? `<div class="meeting-block"><div class="meeting-block-label">Summary</div><p class="meeting-summary">${esc(note.summary)}</p></div>` : ""}
      ${items.length ? `<div class="meeting-block"><div class="meeting-block-label">Action items</div><ul class="meeting-actions-list">${items.map(item => `<li>${esc(item)}</li>`).join("")}</ul></div>` : ""}
      ${hasTranscript ? `<details class="meeting-transcript" data-meeting-transcript="${esc(id)}"${isOpen ? " open" : ""}><summary>Transcript</summary><p class="meeting-transcript-text">${esc(note.transcript)}</p></details>` : ""}
      <div class="actions meeting-note-actions">
        <button type="button" class="btn sm" data-meeting-copy-summary="${esc(id)}"${!hasSummary && !items.length && !hasTranscript ? " disabled" : ""}>Copy summary</button>
        <button type="button" class="btn sm quiet" data-meeting-copy-transcript="${esc(id)}"${!hasTranscript ? " disabled" : ""}>Copy transcript</button>
        <span class="spacer"></span>
        <button type="button" class="btn sm ghost" data-meeting-delete="${esc(id)}">Delete</button>
      </div>
    </article>`;
  }).join("");

  $$("[data-meeting-transcript]", listEl).forEach(det => det.ontoggle = () => {
    const id = det.dataset.meetingTranscript;
    if (det.open) openMeetingTranscripts.add(id);
    else openMeetingTranscripts.delete(id);
  });
  $$("[data-meeting-copy-summary]", listEl).forEach(btn => btn.onclick = () => {
    const note = meetingState.notes.find(n => String(n.id) === btn.dataset.meetingCopySummary);
    if (note) copySafeText(formatMeetingSummaryCopy(note), "Meeting summary copied.");
  });
  $$("[data-meeting-copy-transcript]", listEl).forEach(btn => btn.onclick = () => {
    const note = meetingState.notes.find(n => String(n.id) === btn.dataset.meetingCopyTranscript);
    if (note) copySafeText(note.transcript || "", "Meeting transcript copied.");
  });
  $$("[data-meeting-delete]", listEl).forEach(btn => btn.onclick = () => deleteMeetingNote(btn.dataset.meetingDelete));
}
async function loadMeetings(immediate = false) {
  if (immediate) stopMeetingsPoll();
  if (meetingsPollInFlight) return;
  if (typeof api().get_meetings !== "function") return;
  meetingsPollInFlight = true;
  try {
    const data = await api().get_meetings();
    renderMeetings(data);
  } catch { /* retry on next poll if active or on Meetings page */ }
  finally {
    meetingsPollInFlight = false;
    scheduleMeetingsPoll(false);
  }
}
async function deleteMeetingNote(id) {
  if (!await askConfirm("Delete this meeting note? This cannot be undone.", { title: "Delete meeting note?", confirmLabel: "Delete", destructive: true })) return;
  try {
    const res = await api().delete_meeting(Number(id));
    if (res?.ok) {
      openMeetingTranscripts.delete(String(id));
      toast("Meeting note deleted.");
      await loadMeetings(true);
    } else {
      toast(res?.message || "Could not delete meeting note.", true);
    }
  } catch {
    toast("Could not delete meeting note.", true);
  }
}
$("#btn-meeting-start")?.addEventListener("click", async () => {
  if (meetingActionPending || typeof api().start_meeting !== "function") return;
  meetingActionPending = true;
  renderMeetings(meetingState);
  try {
    const res = await api().start_meeting();
    if (res?.ok) {
      if (res.loopback_warning) toast(res.loopback_warning);
      else toast("Meeting recording started.");
      await loadMeetings(true);
    } else {
      toast(res?.message || "Could not start meeting recording.", true);
    }
  } catch {
    toast("Could not start meeting recording.", true);
  } finally {
    meetingActionPending = false;
    renderMeetings(meetingState);
  }
});
$("#btn-meeting-stop")?.addEventListener("click", async () => {
  if (meetingActionPending || typeof api().stop_meeting !== "function") return;
  meetingActionPending = true;
  renderMeetings(meetingState);
  try {
    const res = await api().stop_meeting();
    if (res?.ok) {
      toast("Stopping and summarizing locally…");
      await loadMeetings(true);
    } else {
      toast(res?.message || "Could not stop meeting recording.", true);
    }
  } catch {
    toast("Could not stop meeting recording.", true);
  } finally {
    meetingActionPending = false;
    renderMeetings(meetingState);
  }
});
$("#btn-meeting-cancel")?.addEventListener("click", async () => {
  if (meetingActionPending || typeof api().cancel_meeting !== "function") return;
  meetingActionPending = true;
  renderMeetings(meetingState);
  try {
    const res = await api().cancel_meeting();
    if (res?.ok) {
      toast("Meeting recording canceled.");
      await loadMeetings(true);
    } else {
      toast(res?.message || "Could not cancel meeting recording.", true);
    }
  } catch {
    toast("Could not cancel meeting recording.", true);
  } finally {
    meetingActionPending = false;
    renderMeetings(meetingState);
  }
});

// ---------- command palette ----------
const commands = () => {
  const themeButtons = $$("#appearance-swatches .theme-swatch");
  const layoutButtons = $$("#appearance-layouts .layout-card");
  return [
    { l: "Go to Dictate", k: "Ctrl+1", f: () => show("dictate") }, { l: "Go to Prompt Lab", k: "Ctrl+2", f: () => show("lab") },
    { l: "Go to Meetings", k: "Ctrl+3", f: () => show("meetings") }, { l: "Go to History", k: "Ctrl+4", f: () => show("history") },
    { l: "Go to Settings", k: "Ctrl+5", f: () => show("settings") },
    { l: "Improve prompt", f: () => $("#btn-improve").click() }, { l: "Get feedback", f: () => $("#btn-score").click() },
    { l: "Paste draft", k: "Ctrl+Enter", f: () => $("#btn-paste").click() }, { l: "Save favorite", k: "Ctrl+S", f: () => $("#btn-save").click() }, { l: "New draft", f: () => $("#btn-new").click() },
    { l: "Change dictation hotkey", f: () => openHotkeyPicker(document.activeElement) },
    { l: `AI formatting: ${settings.cleanup ? "turn off" : "turn on"}`, f: () => setSetting("cleanup", !settings.cleanup) },
    { l: `App-aware tone: ${settings.auto_app_tone ? "turn off" : "turn on"}`, f: () => setSetting("auto_app_tone", !settings.auto_app_tone) },
    { l: `Quiet / whisper mode: ${settings.quiet_mode ? "turn off" : "turn on"}`, f: () => setSetting("quiet_mode", !settings.quiet_mode) },
    { l: `Sound cues: ${settings.sounds ? "turn off" : "turn on"}`, f: () => setSetting("sounds", !settings.sounds) },
    { l: `Floating recording card: ${settings.floating_card ? "turn off" : "turn on"}`, f: () => setSetting("floating_card", !settings.floating_card) },
    { l: `Restore clipboard after paste: ${settings.restore_clipboard ? "turn off" : "turn on"}`, f: () => setSetting("restore_clipboard", !settings.restore_clipboard) },
    { l: "Check for better local models", f: () => { show("settings", false); setSettingsTab("models", { skipAutoCheck: true }); checkForBetterModels(false); } },
    { l: "Refresh local Ollama models", f: () => { const alreadyRendered = settingsRendered; show("settings", false); setSettingsTab("models", { skipAutoCheck: true }); if (alreadyRendered && !modelsRefreshPending) refreshModels(); } },
    ...["destination", "target", "engine", "judge", "judge_model", "style"].flatMap(k => optionEntries(k).map(([n, v]) => ({ l: `${SETTING_ROWS.find(r => r[0] === k)[1]}: ${n}`, f: () => setSetting(k, v) }))),
    ...themeButtons.map(b => ({ l: `Theme: ${b.textContent.trim()}`, f: () => b.click() })),
    ...layoutButtons.map(b => ({ l: `Layout: ${$(".layout-card-title", b)?.textContent || b.dataset.layoutChoice}`, f: () => b.click() })),
  ];
};
let palSel = 0, paletteOpener = null, paletteItems = [];
function updatePaletteSelection() {
  $$("#palette-list li").forEach((li, i) => {
    const selected = i === palSel;
    li.classList.toggle("sel", selected);
    li.setAttribute("aria-selected", String(selected));
  });
  const input = $("#palette-input"), selected = $("#palette-list .sel");
  if (selected) {
    input.setAttribute("aria-activedescendant", selected.id);
    selected.scrollIntoView({block: "nearest"});
  } else input.removeAttribute("aria-activedescendant");
}
function renderPalette() {
  const q = $("#palette-input").value.toLowerCase();
  const items = commands().filter(c => c.l.toLowerCase().includes(q)).slice(0, 12);
  paletteItems = items;
  palSel = Math.min(Number.isFinite(palSel) ? palSel : 0, Math.max(0, items.length - 1));
  $("#palette-list").innerHTML = items.map((c, i) => `<li id="palette-option-${i}" role="option" aria-selected="${i === palSel}" class="${i === palSel ? "sel" : ""}" data-i="${i}"><span>${esc(c.l)}</span><span class="muted">${c.k || ""}</span></li>`).join("");
  updatePaletteSelection();
  $("#palette-empty").classList.toggle("hidden", !!items.length);
  $$("#palette-list li").forEach(li => li.onclick = () => { const command = items[+li.dataset.i]; closePalette(); command.f(); });
  return items;
}
function openPalette() { paletteOpener = document.activeElement; $("#palette-input").value = ""; palSel = 0; renderPalette(); $("#palette").showModal(); updatePaletteSelection(); $("#palette-input").setAttribute("aria-expanded", "true"); $("#palette-input").focus(); }
function closePalette() { $("#palette").close(); $("#palette-input").setAttribute("aria-expanded", "false"); if (paletteOpener?.isConnected) paletteOpener.focus(); }
$("#palette").addEventListener("cancel", ev => { ev.preventDefault(); closePalette(); });
$("#palette-input").oninput = () => { palSel = 0; renderPalette(); };
$("#palette").onclick = ev => { if (ev.target.id === "palette") closePalette(); };

// ---------- keyboard ----------
document.addEventListener("keydown", ev => {
  if (ev.defaultPrevented || ev.isComposing || ev.keyCode === 229) return;
  const pal = $("#palette").open;
  if(!pal && document.querySelector('dialog[open]')) return;
  if (pal) {
    const items = paletteItems;
    if (ev.key === "ArrowDown") { if(items.length)palSel = (palSel + 1) % items.length; updatePaletteSelection(); ev.preventDefault(); }
    else if (ev.key === "ArrowUp") { if(items.length)palSel = (palSel - 1 + items.length) % items.length; updatePaletteSelection(); ev.preventDefault(); }
    else if (ev.key === "Enter") { const command=items[palSel]; if(command){closePalette();command.f();} ev.preventDefault(); }
    else if (ev.key === "Escape") {closePalette();ev.preventDefault();}
    return;
  }
  if (ev.ctrlKey && ev.key >= "1" && ev.key <= "5") { show(["dictate", "lab", "meetings", "history", "settings"][+ev.key - 1]); ev.preventDefault(); }
  else if (ev.ctrlKey && ev.key.toLowerCase() === "k") { openPalette(); ev.preventDefault(); }
  else if (ev.ctrlKey && !ev.altKey && !ev.metaKey && !ev.shiftKey && ev.key === "Enter" && page === "lab") { $("#btn-paste").click(); ev.preventDefault(); }
  else if (ev.ctrlKey && ev.key.toLowerCase() === "s" && page === "lab") { $("#btn-save").click(); ev.preventDefault(); }
  else if (ev.key === "Escape") { closeMore(true); $("#library").classList.add("hidden"); }
});

// ---------- polling ----------
function setDisplayedProperty(element, property, value) {
  if (element[property] !== value) element[property] = value;
}
// Under 600px tall the banners collapse to one summary line; syncBanners keeps that line current.
function syncBanners() {
  const shown = $$("#banners > .notice, #banners > .card").filter(el => !el.classList.contains("hidden")), first = shown[0];
  $("#banners-bar").classList.toggle("hidden", !shown.length);
  if (!shown.length && $("#banners").classList.contains("open")) setBannersOpen(false);
  const text = first ? (first.classList.contains("notice") ? first.textContent : first.querySelector(".card-title")?.textContent || "") : "";
  setDisplayedProperty($("#banners-summary"), "textContent", text + (shown.length > 1 ? ` (+${shown.length - 1} more)` : ""));
}
let speechStartingSince = null, speechRetryPending = false;
function setStatus(st) {
  const speech = st.speech_runtime || {};
  const speechFailed = speech.state === "failed" || (st.state === "loading" && !!st.last_error);
  const previewRecovery = speech.reason === "preview_recovery_pending";
  const speechStarting = !speechFailed && (st.state === "loading" || speech.state === "loading" ||
    (speech.state === "switching" && !previewRecovery && speech.reason !== "idle_unloaded"));
  const now = Date.now();
  speechStartingSince = speechStarting ? speechStartingSince ?? now : null;
  const speechSlow = speechStarting && now - speechStartingSince >= 15000;
  const s = st.state === "recording" ? "recording" : st.busy || (speechFailed || st.last_error ? "error" : speechStarting ? "loading" : "idle");
  const recordingStarting = s === "recording" && st.recording_ready === false;
  const stoppingPreview=st.state!=="recording"&&st.preview_busy&&!st.busy;
  const cls = stoppingPreview || previewRecovery && s === "idle" ? "busy" : s === "recording" ? recordingStarting ? "busy" : "recording" : s === "idle" ? "idle" : s === "error" ? "error" : "busy";
  const recordingLabel = st.recording_ready == null ? "recording" : st.recording_ready ? "listening" : "starting…";
  const label = stoppingPreview ? st.pending_dictations>0 ? "finishing dictation…" : "stopping preview…" : s === "recording" ? recordingLabel + (st.locked ? " · hands-free" : "") : s === "loading" ? speechSlow ? "Taking longer" : "Starting speech" : s === "error" ? speechFailed ? "Speech unavailable" : "Error" : s === "idle" && previewRecovery ? "Speech needs attention" : s;
  const keyName = hotkeyLabel(settings.hotkey);
  setDisplayedProperty($("#dictate-heading"), "textContent", st.paste_waiting ? "Waiting for the original app" : s === "recording" ? recordingStarting ? "Starting…" : "Listening" : s === "loading" ? "Getting ready…" : s === "error" ? "Needs your attention" : st.busy || stoppingPreview ? "Finishing your words…" : "Ready when you are");
  const readyInstruction = originalPicture ? `Hold ${keyName} to speak. Release to send your words, or tap for hands-free.` : `Press ${keyName} to start. Press it again to finish.`;
  setDisplayedProperty($("#dictate-instruction"), "textContent", st.paste_waiting ? "Finishing paste…" : s === "recording" ? recordingStarting ? "Opening your microphone. Wait for the ready cue." : st.locked ? `Press ${keyName} or choose Finish recording. Esc cancels.` : "Release the hotkey to finish. Esc cancels." : s === "loading" ? "Wait for speech to be ready." : s === "error" ? st.last_error || "Check the speech status above." : readyInstruction);
  if (originalPicture) {
    setDisplayedProperty(pictureNavStatus, "className", "dot nav-dot " + cls);
    setDisplayedProperty(pictureCleanup, "textContent", "AI clean-up " + (!settings.cleanup ? "off" : st.llm_ok ? "on" : "offline"));
  }
  for (const el of $$(".status")) {
    setDisplayedProperty($(".dot", el), "className", "dot " + cls);
    const showCleanupStatus = !originalPicture || el.id !== "status-dictate";
    setDisplayedProperty($(".status-text", el), "textContent", label + (showCleanupStatus && settings.cleanup && !st.llm_ok && s === "idle" ? st.cleanup_warming ? " · AI formatting loading…" : " · AI formatting offline" : ""));
  }
  const announcement=$("#recording-announcement");if(announcement.textContent!==label)announcement.textContent=label;
  document.body.classList.toggle("recording", s === "recording");
  document.body.classList.toggle("recording-starting", recordingStarting);
  setDisplayedProperty($("#btn-finish"), "disabled", s!=="recording");
  setDisplayedProperty($("#btn-cancel"), "disabled", s!=="recording");
  $("#btn-finish").classList.toggle("primary", s==="recording");
  const speechNotice=$("#speech-runtime-status"), speechBanner=$("#speech-notice");
  const retryAllowed = st.speech_retry_ready ?? (st.state === "loading" && !!st.last_error);
  const speechMessage=previewRecovery ? "Live preview paused. Speech will recover when you finish recording, or choose Retry startup when idle."
    : speechFailed ? retryAllowed ? "Speech couldn’t start. Try starting it again." : "Speech is unavailable. Wait for current work to finish before trying again."
    : speechSlow ? "Speech is taking longer to start. Listen is still waiting for it."
    : speechStarting ? speech.device === "cpu" && speech.fallback ? "Switching speech to your computer's processor. Please wait." : "Loading local speech. You can keep using the window."
    : speech.fallback ? "Speech is using your computer's processor because GPU acceleration is unavailable. Your selected speech model is unchanged; dictation may take longer." : "";
  speechBanner.classList.toggle("hidden", !speechMessage);
  speechBanner.classList.toggle("warning", speechSlow || previewRecovery);
  speechBanner.classList.toggle("error", speechFailed);
  speechNotice.classList.toggle("hidden",!speechMessage);
  if(speechNotice.textContent!==speechMessage)speechNotice.textContent=speechMessage;
  $("#btn-retry").classList.toggle("hidden", !(speechFailed || previewRecovery));
  setDisplayedProperty($("#btn-retry"), "disabled", !retryAllowed || speechRetryPending);
  const speechDiagnostic=$("#diag-speech");
  if(speechDiagnostic&&st.speech_summary&&speechDiagnostic.textContent!==st.speech_summary)speechDiagnostic.textContent=st.speech_summary;
  const speechStatus = $("#speech-model-status");
  speechRuntimeBusy = !!st.speech_model_switching || st.state === "recording" || !!st.busy || !!st.lab_busy || !!st.pending_dictations || !!st.preview_busy || (st.state === "loading" && !st.last_error);
  const speechDisabled = speechModelSavePending || speechRuntimeBusy || !!modelCatalog?.install?.active;
  for (const btn of $$("#speech-catalog-list [data-use-speech]")) {
    setDisplayedProperty(btn, "disabled", speechDisabled);
  }
  if (speechStatus) {
    if (st.speech_model_switching || speechModelSavePending) setDisplayedProperty(speechStatus, "textContent", "Changing speech model…");
    else if (!speechRuntimeBusy) setDisplayedProperty(speechStatus, "textContent", speechOptionsError);
  }
  const speechSummary = $("#speech-stage-summary");
  if (speechSummary && st.speech_summary && (st.state === "loading" || st.state === "error" || !modelCatalog?.speech)) {
    setDisplayedProperty(speechSummary, "textContent", st.speech_summary);
  }
  syncBanners();
}
let lastErr = "", retainedVersion = -1;
async function loadRetained() {
  if (!api().get_retained) return;
  const rows = await api().get_retained(), box = $("#retained-results");
  box.classList.toggle("hidden", !rows.length);
  $("#retained-list").innerHTML = rows.map(r => `<div class="retained-item"><p>${esc(r.message)}</p><textarea class="input" readonly aria-label="Unpasted text">${esc(r.text)}</textarea><button class="btn" data-copy-retained="${esc(r.id)}">Copy text</button> <button class="btn" data-dismiss-retained="${esc(r.id)}">Dismiss</button></div>`).join("");
  $$('[data-copy-retained]').forEach(b => b.onclick = async () => { try { if(await api().copy_retained(+b.dataset.copyRetained)) toast("Copied. Paste into your intended app."); else toast("This result changed or could not be copied. Your visible text remains available to select and copy.",true); } catch { toast("Copy was not confirmed. Your text remains here; reconnect and try again.",true); } });
  $$('[data-dismiss-retained]').forEach(b => b.onclick = async () => { try { if(!await api().dismiss_retained(+b.dataset.dismissRetained))toast("Dismissal could not be saved. The recovery text is preserved.",true); await loadRetained(); } catch { toast("Dismissal was not confirmed. The recovery text remains available.",true); } });
}
let manualClipboardReceipt=null;
async function loadDeliveryStatus() {
  if(!api().get_delivery_status)return;
  const state=await api().get_delivery_status();
  manualClipboardReceipt=state.manual_id || null;
  $("#delivery-recovery").classList.toggle("hidden",!state.pending);
  setDisplayedProperty($("#delivery-title"), "textContent", manualClipboardReceipt ? "Previous clipboard item kept" : "Paste needs confirmation");
  setDisplayedProperty($("#delivery-description"), "textContent", manualClipboardReceipt
    ? "Your previous clipboard item is kept here. Normal dictation can continue. After pasting your text, restore the previous item or choose to keep your current clipboard. Keep Listen open until you decide."
    : "Check the destination before copying again; the text may already be there. New dictations are saved in recovery while this paste is unresolved. Keep Listen open to preserve your previous clipboard.");
  setDisplayedProperty($("#delivery-message"), "textContent", state.message || "");
  $$('[data-copy-retained]').forEach(b=>b.disabled=!!state.pending&&!manualClipboardReceipt);
}
$("#restore-clipboard").onclick=async()=> {
  const receipt=manualClipboardReceipt;
  if(!await askConfirm(receipt ? "After pasting your dictation, restore your previous clipboard item?" : "First wait for the receiving app to finish pasting, or close that app. Restore your previous clipboard only when no paste is still waiting; otherwise a delayed paste could insert old text."))return;
  try {const result=await api().restore_delivery_clipboard(true,receipt);toast(result.message,result.status!=="restored");await loadDeliveryStatus();}
  catch {toast("Clipboard restoration was not confirmed. The saved clipboard remains in memory; keep Listen open.",true);}
};
$("#discard-clipboard").onclick=async()=> {
  const receipt=manualClipboardReceipt;
  if(!await askConfirm(receipt ? "Keep your current clipboard and discard the saved previous item?" : "First finish or cancel the waiting paste, or close the receiving app. Keep your current clipboard and permanently discard the saved previous clipboard?"))return;
  try {const result=await api().discard_delivery_clipboard(true,receipt);toast(result.message,result.status!=="resolved");await loadDeliveryStatus();}
  catch {toast("Clipboard recovery is still pending. Keep Listen open and try again.",true);}
};
async function poll() {
  pollRunning=true;
  const slowConnection = watchConnection();
  const statePreviewEpoch=previewUiEpoch;
  let controllerAttempt;
  try {
    const bridge = api();
    if (typeof bridge.begin_controller_hotkey_capture === "function") controllerAttempt = controllerHotkeyAttempt;
    S = await bridge.get_state(); bridgeLost=false;
  } catch (e) {
    bridgeLost=true;
  }
  if (!bridgeLost) try {
    if (controllerAttempt) receiveControllerHotkey(controllerAttempt, S.controller_hotkey_capture);
    setStatus(S);
    renderPreview(S,statePreviewEpoch);
    if(!S.busy && S.state!=="recording" && Date.now()-lastDeliveryAt>=1000) { lastDeliveryAt=Date.now(); await loadDeliveryStatus(); }
    if (S.retained_ver !== retainedVersion) { await loadRetained(); retainedVersion = S.retained_ver; }
    if (S.levels && S.levels.length) drawBars(S.levels);
    else if (!document.body.classList.contains("recording") && (page === "dictate" || page === "lab")) drawBars([]);
    if (S.lab_ver !== lastLabVer) { if (await loadLab() !== false) lastLabVer = S.lab_ver; }  // an aborted load is retried next tick
    if (S.history_len !== lastHistLen) { lastHistLen = S.history_len; loadRecent(); loadSuggestions(); if (page === "history") loadHistory(); }
    if (S.event === "show_lab") show("lab");
    if (S.event === "pasted") toast("Pasted");
    if (S.event === "sent") toast("Text sent");
    if (S.last_error && S.last_error !== lastErr) { lastErr = S.last_error; toast(S.last_error, true); }
  } catch (e) { toast("Could not refresh part of the screen. Your input is preserved.", true); }
  clearTimeout(slowConnection);
  if (bridgeLost) connection("Desktop connection lost. Check again to see whether Listen is responding.", checkConnection);
  else connection("");
  pollRunning=false; pollTimer=setTimeout(poll, bridgeLost ? 1000 : pollDelay());
}
let lastDeliveryAt=0, pollTimer=0, pollRunning=false;
const pollDelay = () => document.hidden ? 1000 : document.body.classList.contains("recording") ? 33 : 250;
function watchConnection() {
  return setTimeout(() => connection("Listen is taking longer to respond. If it stays here, quit from the tray and reopen Listen after any recording finishes."), 5000);
}
function checkConnection() {
  connection("Checking desktop connection…");
  if (pollRunning) return;
  clearTimeout(pollTimer);
  if (booted) poll();
  else {
    bridgeReady ||= !!window.pywebview?.api;
    boot();
  }
}

// ---------- boot ----------
for(const [id,method] of [["finish","finish_recording"],["cancel","cancel_recording"]]) $("#btn-"+id).onclick=async()=> {previewActionPending++;invalidatePreview();try{if(!await api()[method]())toast("Action unavailable. Check status and try again.",true);}catch{connection("Desktop connection lost. Your text is preserved.");}finally{previewActionPending--;invalidatePreview();}};
$("#btn-retry").onclick = async () => {
  if (speechRetryPending) return;
  speechRetryPending = true;
  $("#btn-retry").disabled = true;
  try {
    if (!await api().retry_startup()) toast("Speech can’t retry yet. Check the status and try again.", true);
  } catch { connection("Desktop connection lost. Check again to see whether Listen is responding.", checkConnection); }
  finally { speechRetryPending = false; }
};
let initialConnectionTimer;
async function boot() {
  if(booted)return;booted=true;
  clearTimeout(initialConnectionTimer);
  const slowConnection = watchConnection();
  $("#demo-banner").classList.toggle("hidden",!demo);
  const version = vocabVersion;
  try {
    settings = await api().get_settings();
    options = await api().get_options();
    if (!vocabDirty && vocabVersion === version) vocab.value = settings.vocab || "";
    if (originalPicture && vocab.value) $("#vocab-status").textContent = "Vocabulary saved locally";
    renderChips();
    if (!$('.page.active')) show(page, false);
    else if (page === "settings") renderSettings();
    else if (page === "history") loadHistory(true);
    else if (page === "meetings") loadMeetings(true);
    poll();
  }
  catch{booted=false;connection("Could not connect to the desktop app. Check again to reconnect.", checkConnection);}
  finally { clearTimeout(slowConnection); }
}
// Hidden windows poll slowly; coming back restores the normal rate at once (unless a poll is already running).
document.addEventListener("visibilitychange",()=> {
  if(!document.hidden&&pollTimer&&!pollRunning) { clearTimeout(pollTimer); poll(); }
  if(!document.hidden&&shouldPollMeetings()&&!meetingsPollInFlight) loadMeetings(true);
});
function setBannersOpen(open) {
  $("#banners").classList.toggle("open", open);
  $("#banners-toggle").setAttribute("aria-expanded", String(open));
  $("#banners-toggle").textContent = open ? "Hide" : "Details";
}
$("#banners-toggle").onclick = () => setBannersOpen(!$("#banners").classList.contains("open"));
const metricArea = $("#stats");
metricArea.addEventListener("pointerout", event => {
  const card = event.target.closest(".stat");
  if (card && !card.contains(event.relatedTarget)) delete card.dataset.tipDismissed;
});
metricArea.addEventListener("focusout", event => {
  const card = event.target.closest(".stat");
  if (card && !card.matches(":hover")) delete card.dataset.tipDismissed;
});
document.addEventListener("keydown", event => {
  if (event.key !== "Escape" || page !== "history") return;
  for (const card of metricArea.querySelectorAll(".stat:hover, .stat:focus")) card.dataset.tipDismissed = "true";
});
$("#lab-models").addEventListener("toggle", async event => {
  if (event.newState !== "open") return;
  try {
    const data = await api().get_local_models();
    if (page === "lab") rememberPromptModels(data);
  } catch { if (page === "lab") rememberPromptModels({models:[],available:false}); }
});
window.addEventListener("pywebviewready",()=>{bridgeReady=true;boot();},{once:true});
if(demo || bridgeReady)boot();else {
  connection("Connecting to desktop app…");
  initialConnectionTimer = setTimeout(() => connection("Still waiting for the Listen desktop app.", checkConnection), 5000);
}
})();
