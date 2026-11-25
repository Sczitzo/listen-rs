# Listen Rust mechanics

The reference baseline is the original Python/JS [Listen](https://github.com/Sczitzo/Listen) project (MIT, by Sczitzo).
Listen v2026.11.21 (public revision `bebab8be2acc36794d66cb89fdc14c6d724822a3`) is the historical parity checkpoint.
Numeric values and their origins are in `mechanics.json`. Having a number in the sheet does not measure it.

## Repeating the comparison

From the repository root, run:

```powershell
cargo build --release --locked
python tools/check_mechanics.py --report mechanics-new-report.json
```

Use a new report filename each time. The checker runs the release executable
with controlled sample audio and simulated timings, then compares its measured
values with the sheet. It exits with code 0 when every measured value matches
and 1 for a difference or an error. The report records the executable and
sheet hashes, expected values, observed values, and the method for each
measurement.

The measured timing checks exercise the shortcut's own decision rules at
one-millisecond resolution. The recording checks feed samples through Listen's
finished-recording logic, resampler and speech chunking. Chime checks measure
generated speaker samples. Physical keyboard, microphone and speaker behavior
require a live test. Craft Anything's `rebuild_check` checks the build, startup
and sheet separately. Its mechanics status remains incomplete.

## Recording and delivery

- Original dictate.py:448-463 (ev_f41821b6) sets Right Ctrl, a tap under 0.30 s
  for hands-free, a hold cue after 0.15 s, 600 s maximum, 16 kHz mono, and a
  silence threshold of 0.004. Rust uses these settings. Windows shortcut
  selections can use a registered combination or an Xbox-compatible digital
  button. The chooser lease is 60 seconds. An unmatched registered press
  keeps its transcript for manual delivery. A controller needs one connected
  and neutral pad, and a distinct button press.
- The original `dictate.py:1944-1949` reports a microphone error when a recording
  lasts longer than 0.5 seconds and receives no audio. With identical empty
  inputs, the original and rebuild treated 100 ms and 500 ms as no speech and
  reported the microphone error at 501 ms and 2 seconds. The rebuild also
  treats 2 seconds of captured silence as no speech. This test supplies audio
  samples directly and does not exercise a physical microphone.
- The audio path divides recognition into chunks up to 20 seconds, finds
  a quiet 1,600-sample window near the end, and joins the resulting text.
  The 4-thread CPU Parakeet runtime uses the locally bundled model.
- Dictation clean-up is local and optional. The original Qwen default is
  qwen2.5:7b, its context is 8,192, it stays warm for five minutes, and its
  fallback budget is 15 seconds. Before text is sent to Ollama, the rebuild
  checks that the selected model is local. Plain speech is retained when
  clean-up fails or would change too much.
- The rebuild saves History before attempting input. Single-line and multiline
  text use one direct replacement in the captured standard Windows Edit field.
  Exact readback confirms delivery. Unsupported fields use manual recovery.
  If readback fails, check the original field before copying the saved text.
  Rust keeps the original destination and makes one delivery attempt. History
  can group by date and app. Destination names are bounded to 80 characters and
  the optional executable icon is 128 pixels.
- Original chime data at dictate.py:783-788 (ev_7ebedf21) specifies 660/880 Hz
  for Start, 880/660 Hz for Stop, 440 Hz for Cancel, 330/330 Hz for Error,
  880 Hz for Copied, and a default 90 ms note. Rust follows those notes.

## Speech library inspection

The original dashboard and dictation logic are available as Python, HTML and
JavaScript. The Parakeet native runtime is transcribe.dll 0.4.2, SHA-256
bfe89e1ad7c126c51f0774133f3744f75c1b79829bd8cc2527ffbd172f4cf3b3.
The earlier deep scan and this run used the same DLL.

| Address | Observed rule |
|---|---|
| 0x180014a60 | Model parameter block is 32 bytes. |
| 0x180012fc0 | Blocks under 24 bytes fail with status 14. Backend at byte 8 is 0-6. |
| 0x180014ce0 | Session parameter block is 24 bytes. |
| 0x18001c090 | Optional run parameter block is 104 bytes. `timestamps` at byte 12 defaults to `1` (automatic). `spec_k_drafts` at byte 64 (`0x40`) defaults to `-1` (model default). |
| 0x180018ca0 | Null parameters use defaults. A caller block under 68 bytes fails with status 14 and a larger block is bounded to 104 bytes. |
| 0x180018840 | A non-finite sample rejects the chunk with status 1. Rust replaces such samples with silence. |
| 0x18001b670 | Full-text access returns empty text for an empty result. |
| 0x180014bb0 | Export maps statuses 0-21 to messages. The bundled function returned the same messages when called with each code. |

DLL evidence includes ev_c5157e1d (run defaults), ev_146e8a6f (task and
runtime checks), and ev_faeef3ca (status table). The bundled DLL's initializer
returned the size and defaults shown above. Automatic timestamps request the
finest timing detail compatible with the model and task. The draft setting
controls a decoding speed feature for models that support it. The optional
source-language hint defaults to unset. [Speech library v0.4.2 settings](https://github.com/handy-computer/transcribe.cpp/blob/v0.4.2/include/transcribe.h#L1072-L1190)

Listen ships an English-only model and uses the library defaults for each run.
It reads back the transcript text and sets the CPU thread count separately
when opening the speech session. The app defaults to four threads.

## Dashboard and Lantern

- The Rust dashboard uses embedded HTML/CSS/JS assets in Wry
  0.57.3 and Windows WebView2. Rust handles settings, capture, local storage,
  and the native Lantern. The dashboard exposes twelve WCAG AA-verified color
  schemes and three layouts (Writer's desk, Sidebar, and Compact) in
  Appearance. The normal
  window starts at 900 x 700 points, or 480 x 760 in Compact. Preview
  captures use approximately 120 frames and leave focus with the user.
- Current Dictate has the shortcut picker and one status indicator. Speech
  is labelled slow after 15 seconds, desktop connection after five seconds.
  The compact Lantern is 380 pixels wide and draws in the work area of the
  destination app. The original can also polish provisional text during
  recording (ev_3212d1a4). Rust shows provisional speech without that extra
  model pass.
- History uses the release's Day and App grouping and 1.5-second digit roll.
  The Rust processing metric covers speech and clean-up. It has separate
  storage from Python Listen. Data import requires a separate decision and
  test with safe copies.
- Prompt Lab allows independent local, Claude, OpenAI, and Gemini routing
  for writing and checking. Cloud needs the paid-cloud setting and an
  environment variable. Requests are counted as unpriced attempts. A result
  is still a suggestion to review. Full live multi-pass review is open.

See the upgrade record for verification and limits.
