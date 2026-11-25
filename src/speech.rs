//! Speech-to-text. Two engines:
//! 1. The same native runtime the original uses: transcribe.dll 0.4.2 with the
//!    Parakeet Unified 0.6B Q8_0 model, loaded directly (no Python in between).
//! 2. A whisper.cpp command-line program, if the person points us at one.

use crate::storage::{Settings, SpeechChoice};
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::{Path, PathBuf};

pub const SAMPLE_RATE: usize = 16_000;

pub enum Speech {
    Dll(DllEngine),
    Cli { exe: PathBuf, model: PathBuf, threads: i32 },
}

impl Speech {
    pub fn label(&self) -> String {
        match self {
            Speech::Dll(_) => "Parakeet 0.6B · CPU".into(),
            Speech::Cli { model, .. } => format!(
                "whisper.cpp · {}",
                model.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
            ),
        }
    }

    pub fn transcribe(&mut self, samples: &[f32], vocab: &str) -> Result<String, String> {
        match self {
            Speech::Dll(e) => {
                let mut parts = Vec::new();
                for chunk in split_chunks(samples) {
                    if rms(chunk) < 1e-4 {
                        continue;
                    }
                    let t = e.run(chunk)?;
                    if !t.is_empty() {
                        parts.push(t);
                    }
                }
                Ok(parts.join(" "))
            }
            Speech::Cli { exe, model, threads } => run_cli(exe, model, *threads, samples, vocab),
        }
    }
}

pub fn rms(s: &[f32]) -> f32 {
    if s.is_empty() {
        return 0.0;
    }
    (s.iter().map(|x| (*x as f64) * (*x as f64)).sum::<f64>() / s.len() as f64).sqrt() as f32
}

/// Loudest 100 ms window, used to decide whether anything was said.
pub fn peak_window_rms(s: &[f32]) -> f32 {
    s.chunks(1600).map(rms).fold(0.0, f32::max)
}

/// Same rule as the original: pieces of at most 20 s, each cut at the quietest
/// 100 ms window in the last 3 s before the limit, with no overlap.
pub fn split_chunks(s: &[f32]) -> Vec<&[f32]> {
    let n = s.len();
    let mut out = Vec::new();
    let mut start = 0;
    while start < n {
        let mut end = (start + 20 * SAMPLE_RATE).min(n);
        if end < n {
            let lo = (start + 15 * SAMPLE_RATE).max(end.saturating_sub(3 * SAMPLE_RATE));
            let (mut best, mut best_e) = (lo, f64::MAX);
            let mut at = lo;
            while at < end {
                let w = &s[at..(at + 1600).min(n)];
                let e = w.iter().map(|x| (*x as f64) * (*x as f64)).sum::<f64>() / w.len().max(1) as f64;
                if e < best_e {
                    best_e = e;
                    best = at;
                }
                at += 1600;
            }
            end = (best + 800).min(n);
        }
        out.push(&s[start..end]);
        start = end;
    }
    out
}

/// Check that an engine's files are there, without loading anything.
pub fn probe(s: &Settings) -> Result<String, String> {
    let dll_ok = || -> Result<String, String> {
        let dir = crate::storage::effective_transcribe_dir(&s.transcribe_dir);
        if !dir.join("transcribe.dll").is_file() {
            return Err("transcribe.dll wasn't found".into());
        }
        if !crate::storage::effective_speech_model(&s.speech_model).is_file() {
            return Err("the Parakeet model file wasn't found".into());
        }
        Ok("Parakeet 0.6B · CPU".into())
    };
    let cli_ok = || -> Result<String, String> {
        if Path::new(&s.whisper_cli).is_file() && Path::new(&s.whisper_model).is_file() {
            Ok("whisper.cpp".into())
        } else {
            Err("no whisper.cpp program is set".into())
        }
    };
    match s.speech_engine {
        SpeechChoice::Transcribe => dll_ok(),
        SpeechChoice::WhisperCli => cli_ok(),
        SpeechChoice::Auto => dll_ok().or_else(|a| cli_ok().map_err(|b| format!("{a}; {b}"))),
    }
}

pub fn load(s: &Settings) -> Result<Speech, String> {
    let dll = || {
        DllEngine::load(
            &crate::storage::effective_transcribe_dir(&s.transcribe_dir),
            &crate::storage::effective_speech_model(&s.speech_model),
            s.speech_threads.clamp(1, 32),
        )
            .map(Speech::Dll)
    };
    let cli = || {
        let exe = PathBuf::from(&s.whisper_cli);
        let model = PathBuf::from(&s.whisper_model);
        if exe.is_file() && model.is_file() {
            Ok(Speech::Cli { exe, model, threads: s.speech_threads.clamp(1, 32) })
        } else {
            Err("no whisper.cpp program and model are set".to_string())
        }
    };
    match s.speech_engine {
        SpeechChoice::Transcribe => dll(),
        SpeechChoice::WhisperCli => cli(),
        SpeechChoice::Auto => dll().or_else(|a| cli().map_err(|b| format!("{a}; {b}"))),
    }
}

#[repr(C)]
struct ModelParams {
    struct_size: u64,
    backend: i32,
    device: *mut c_void,
    no_mmap: bool,
}

#[repr(C)]
struct SessionParams {
    struct_size: u64,
    n_threads: i32,
    kv_type: i32,
    n_ctx: i32,
}

type FnVersion = unsafe extern "C" fn() -> *const c_char;
type FnInitBackends = unsafe extern "C" fn(*const c_char) -> i32;
type FnModelParamsInit = unsafe extern "C" fn(*mut ModelParams);
type FnSessionParamsInit = unsafe extern "C" fn(*mut SessionParams);
type FnModelLoad = unsafe extern "C" fn(*const c_char, *const ModelParams, *mut *mut c_void) -> i32;
type FnSessionInit = unsafe extern "C" fn(*mut c_void, *const SessionParams, *mut *mut c_void) -> i32;
type FnRun = unsafe extern "C" fn(*mut c_void, *const f32, i32, *mut c_void) -> i32;
type FnText = unsafe extern "C" fn(*mut c_void) -> *const c_char;
type FnFree = unsafe extern "C" fn(*mut c_void);
type FnStatus = unsafe extern "C" fn(i32) -> *const c_char;

// Decompiled from transcribe.dll 0.4.2 (sha256 bfe89e1a…):
// - transcribe_model_load_params_init @0x180014a60 writes struct_size 0x20 (32 bytes);
//   the loader @0x180012fc0 refuses sizes below 0x18 (status 14) and backends >= 7.
// - transcribe_session_params_init @0x180014ce0 writes struct_size 0x18 (24 bytes).
// - transcribe_run @0x18001bfe0 -> @0x1800161b0 returns 1 when any sample is NaN or
//   infinite (check @0x180018840), so samples are cleaned first.
// - transcribe_full_text @0x18001b670 returns "" (never NULL) when there's no result.
// - transcribe_status_string @0x180014bb0 names the 22 status codes (0..=0x15).
const DLL_MODEL_PARAMS_SIZE: u64 = 0x20;
const DLL_SESSION_PARAMS_SIZE: u64 = 0x18;

pub struct DllEngine {
    model: *mut c_void,
    session: *mut c_void,
    run: FnRun,
    text: FnText,
    status: Option<FnStatus>,
    session_free: FnFree,
    model_free: FnFree,
    _lib: libloading::Library,
}

// The handles are only ever used behind a Mutex, from one thread at a time.
unsafe impl Send for DllEngine {}

impl DllEngine {
    pub fn load(dir: &Path, model: &Path, threads: i32) -> Result<Self, String> {
        let dll = dir.join("transcribe.dll");
        if !dll.is_file() {
            return Err(format!("transcribe.dll wasn't found in {}", dir.display()));
        }
        if !model.is_file() {
            return Err(format!("the speech model wasn't found at {}", model.display()));
        }
        #[cfg(windows)]
        set_dll_dir(dir);
        unsafe {
            let lib = libloading::Library::new(&dll).map_err(|e| format!("couldn't load transcribe.dll: {e}"))?;
            macro_rules! sym {
                ($t:ty, $n:literal) => {
                    *lib.get::<$t>($n).map_err(|e| {
                        format!("transcribe.dll has no {}: {e}", String::from_utf8_lossy(&$n[..$n.len() - 1]))
                    })?
                };
            }
            let version: FnVersion = sym!(FnVersion, b"transcribe_version\0");
            let init: FnInitBackends = sym!(FnInitBackends, b"transcribe_init_backends\0");
            let mp_init: FnModelParamsInit = sym!(FnModelParamsInit, b"transcribe_model_load_params_init\0");
            let sp_init: FnSessionParamsInit = sym!(FnSessionParamsInit, b"transcribe_session_params_init\0");
            let model_load: FnModelLoad = sym!(FnModelLoad, b"transcribe_model_load_file\0");
            let session_init: FnSessionInit = sym!(FnSessionInit, b"transcribe_session_init\0");
            let run: FnRun = sym!(FnRun, b"transcribe_run\0");
            let text: FnText = sym!(FnText, b"transcribe_full_text\0");
            let session_free: FnFree = sym!(FnFree, b"transcribe_session_free\0");
            let model_free: FnFree = sym!(FnFree, b"transcribe_model_free\0");
            let status: Option<FnStatus> = lib.get::<FnStatus>(b"transcribe_status_string\0").ok().map(|s| *s);
            let why = |code: i32| -> String {
                match status {
                    Some(f) => {
                        let p = f(code);
                        if p.is_null() { format!("status {code}") } else { format!("{} (status {code})", CStr::from_ptr(p).to_string_lossy()) }
                    }
                    None => format!("status {code}"),
                }
            };

            let v = version();
            let vs = if v.is_null() { String::new() } else { CStr::from_ptr(v).to_string_lossy().into_owned() };
            if vs != "0.4.2" {
                return Err(format!("transcribe.dll is version {vs:?}; version 0.4.2 is needed"));
            }
            let cdir = CString::new(dir.to_string_lossy().as_bytes()).map_err(|_| "bad runtime folder name")?;
            if init(cdir.as_ptr()) != 0 {
                return Err("the speech runtime couldn't start its CPU backend".into());
            }
            let mut mp: ModelParams = std::mem::zeroed();
            let mut sp: SessionParams = std::mem::zeroed();
            mp_init(&mut mp);
            sp_init(&mut sp);
            if mp.struct_size != DLL_MODEL_PARAMS_SIZE
                || sp.struct_size != DLL_SESSION_PARAMS_SIZE
                || std::mem::size_of::<ModelParams>() as u64 != DLL_MODEL_PARAMS_SIZE
                || std::mem::size_of::<SessionParams>() as u64 != DLL_SESSION_PARAMS_SIZE
            {
                return Err("transcribe.dll uses settings this app doesn't understand".into());
            }
            mp.backend = 1; // CPU, as the original: never competes for GPU memory.
            sp.n_threads = threads;
            let cmodel = CString::new(model.to_string_lossy().as_bytes()).map_err(|_| "bad model file name")?;
            let mut m: *mut c_void = std::ptr::null_mut();
            let rc = model_load(cmodel.as_ptr(), &mp, &mut m);
            if rc != 0 || m.is_null() {
                return Err(format!("the speech model couldn't be loaded: {}", why(rc)));
            }
            let mut s: *mut c_void = std::ptr::null_mut();
            let rc = session_init(m, &sp, &mut s);
            if rc != 0 || s.is_null() {
                model_free(m);
                return Err(format!("the speech session couldn't start: {}", why(rc)));
            }
            Ok(DllEngine { model: m, session: s, run, text, status, session_free, model_free, _lib: lib })
        }
    }

    fn run(&mut self, chunk: &[f32]) -> Result<String, String> {
        // transcribe_run refuses the whole chunk if one sample is NaN/infinite (@0x180018840).
        let clean: Vec<f32> = chunk.iter().map(|s| if s.is_finite() { *s } else { 0.0 }).collect();
        unsafe {
            // Use the library's per-run defaults. CPU threads are set when the session opens.
            let rc = (self.run)(self.session, clean.as_ptr(), clean.len() as i32, std::ptr::null_mut());
            if rc != 0 {
                let msg = match self.status {
                    Some(f) => {
                        let p = f(rc);
                        if p.is_null() { format!("status {rc}") } else { CStr::from_ptr(p).to_string_lossy().into_owned() }
                    }
                    None => format!("status {rc}"),
                };
                return Err(format!("speech recognition failed: {msg}"));
            }
            let p = (self.text)(self.session);
            if p.is_null() {
                return Err("speech recognition returned nothing".into());
            }
            Ok(CStr::from_ptr(p).to_string_lossy().trim().to_string())
        }
    }
}

impl Drop for DllEngine {
    fn drop(&mut self) {
        unsafe {
            if !self.session.is_null() {
                (self.session_free)(self.session);
            }
            if !self.model.is_null() {
                (self.model_free)(self.model);
            }
        }
    }
}

#[cfg(windows)]
fn set_dll_dir(dir: &Path) {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    unsafe {
        windows_sys::Win32::System::LibraryLoader::SetDllDirectoryW(wide.as_ptr());
    }
}

fn write_wav(path: &Path, samples: &[f32]) -> std::io::Result<()> {
    use std::io::Write;
    let data_len = (samples.len() * 2) as u32;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?; // PCM
    f.write_all(&1u16.to_le_bytes())?; // mono
    f.write_all(&(SAMPLE_RATE as u32).to_le_bytes())?;
    f.write_all(&(SAMPLE_RATE as u32 * 2).to_le_bytes())?;
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        f.write_all(&v.to_le_bytes())?;
    }
    f.flush()
}

fn run_cli(exe: &Path, model: &Path, threads: i32, samples: &[f32], vocab: &str) -> Result<String, String> {
    let wav = std::env::temp_dir().join(format!("listen-rs-{}.wav", std::process::id()));
    write_wav(&wav, samples).map_err(|e| format!("couldn't write audio: {e}"))?;
    let mut cmd = std::process::Command::new(exe);
    let lang = if model.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.contains(".en")) {
        "en"
    } else {
        "auto"
    };
    cmd.arg("-m")
        .arg(model)
        .arg("-f")
        .arg(&wav)
        .args(["-l", lang, "-nt", "-np", "-t"])
        .arg(threads.to_string());
    let prompt = crate::storage::vocab_words(vocab).join(", ");
    if !prompt.is_empty() {
        let bounded: String = prompt.chars().take(500).collect();
        cmd.arg("--prompt").arg(bounded);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // no console window
    }
    let out = cmd.output();
    let _ = std::fs::remove_file(&wav);
    let out = out.map_err(|e| format!("couldn't run whisper.cpp: {e}"))?;
    if !out.status.success() {
        return Err(format!("whisper.cpp failed: {}", String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("")));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" "))
}
