# Decisions - codebase-tuning

## [2026-02-20] Session: ses_385aebde4ffeJ6hKXbpfG81cqF

### Execution Order
- Wave 1 (T1-T5): ALL PARALLEL — pure test extraction, zero behavior risk
- Wave 2 (T6-T9): ALL PARALLEL — deduplication, independent files
- Wave 3 (T10-T12): SEQUENTIAL — Arc<Config> migration, core dependency chain
- Wave 4 (T13-T17): ALL PARALLEL — module decomposition (after T10 Arc<Config> stable)
- Wave 5 (T18-T20): ALL PARALLEL — data flow optimization
- Wave 6 (T21-T23): ALL PARALLEL — cleanup
- Final (F1-F4): ALL PARALLEL — reviews

### Commit Grouping (from plan)
- Wave 1 (T1-T5): ONE commit `refactor(tests): extract test modules from fat source files`
- Wave 2a (T6-T7): `refactor(channels): deduplicate channel factory and tool descriptions`
- Wave 2b (T8-T9): `fix(skills,email): remove production unwrap and clean feature stub`
- Wave 3 (T10-T12): `refactor(config): migrate to Arc<Config> at entry points`
- Wave 4a (T13-T15): `refactor(providers,security,skills): decompose fat mod.rs into sub-modules`
- Wave 4b (T16-T17): `refactor(channels): split imessage and telegram into sub-modules`
- Wave 5a (T18): `perf(providers): Cow<str> for scrub_secret_patterns`
- Wave 5b (T19-T20): `perf(agent,tools): reduce hot-path cloning`
- Wave 6a (T21): `refactor(memory): resolve dead_code suppressions`
- Wave 6b (T22-T23): `refactor(tests): optimize test helpers and dedup`
