/* Recorded local cleanup comparison; data only, no model calls. */
window.LISTEN_MODEL_BENCHMARKS = Object.freeze({
  "kind": "cleanup",
  "recorded": "2026-11-09",
  "sourceSha256": "2728cdacce3c1917ffbd72ffe481f83ce39c99ade3cfae6867e54a86504341d9",
  "method": "39 synthetic phrases per model, one pass. Successful samples passed the app guard and expected-result term checks. Warm timings exclude first load.",
  "limitation": "Historical results from the previous formatter. This update has not been compared across all models. These are cleanup results, not Prompt Lab writing or review scores.",
  "models": [
    {
      "model": "qwen2.5:7b",
      "cases": 39,
      "passed": 23,
      "accepted": 23,
      "medianSeconds": 0.44,
      "p90Seconds": 0.66
    },
    {
      "model": "qwen3.5:4b",
      "cases": 39,
      "passed": 21,
      "accepted": 22,
      "medianSeconds": 0.46,
      "p90Seconds": 0.6
    },
    {
      "model": "qwen3.5:2b",
      "cases": 39,
      "passed": 23,
      "accepted": 24,
      "medianSeconds": 0.3,
      "p90Seconds": 0.4
    },
    {
      "model": "qwen3.5:9b",
      "cases": 39,
      "passed": 30,
      "accepted": 30,
      "medianSeconds": 0.82,
      "p90Seconds": 1.09
    }
  ]
});
