# rust-run fixture

Committed fixture data for the local Rust CLI smoke and benchmark harness.

The data is synthetic. It must not be replaced with real `~/.codex/sessions`
content, real account identifiers, real cwd values, or real tokens.

`codex-home/codex-ops/usage-mode-history-fast-fixture.json` is not the default
usage mode history path. Tests pass it explicitly so ordinary fixture usage
stays in normal mode unless a test opts into fast attribution.

`codex-home/fast-candidate-sessions` is also outside the default sessions path.
It is a synthetic 5-hour candidate detector fixture. The `used_percent` values
are hand-built across session segments in one 5-hour reset cycle: the baseline
rollout moves `0.0 -> 1.0 -> 2.0 -> 3.0`, the candidate rollout first moves
`3.0 -> 3.0 -> 3.0 -> 9.0` for three gpt-5.4 calls, then
`9.0 -> 9.0 -> 9.0 -> 16.5` for three gpt-5.5 calls. The zero-delta middle
samples model calls that did not immediately change the 5-hour usage percent;
the final sample accumulates the segment's usage. This fixture is only evidence
for detector behavior; it is not a record of a real Codex fast-mode session.
