# Performance and storage implementation results

15 October 2026. Implemented against `a89c81d`, using [the original review](performance-and-storage-review.md) as evidence and the code as the source of truth. All five stages were committed and pushed separately. No deployment, installer invocation, installed service changes, production migrations or operations on existing themed files were performed. The original review was left unchanged.

## Resulting behaviour

Default selection and artwork remain the same: files, folders and generic fallbacks are enabled; rendering stays at 256px. All 857 reachable embedded SVGs retain their original bytes and association rules. All 857 rendered PNGs matched the baseline byte for byte.

Every mutation session uses a shared OS advisory lock acquired before journal reads. The lock releases on process exit and its inode stays in place. The watcher keeps state and an identity index in memory, reloading when an external journal replacement changes its generation. Atomic journal replacement now flushes the file and parent directory. Apply records durable ownership intent before changing icons and checkpoints fingerprints after each batch; undo removes icons before checkpointing each batch of up to 256 records. Unchanged runs do not save. The global Git exclude file is resolved once per process, ownership survives repeated applies and destination changes, and intent is saved before adding the rule. Old owned exclude files remain recorded until cleanup succeeds.

Atomic-save replacements are examined as new objects and themed only if uncustomized. Moves transfer records by identity, including directory descendants, before pruning old paths. macOS can emit unpaired rename events; the identity index handles these. Missing records stay in the journal for at least 30 seconds, with recovery reconciliation before final pruning. Permission/metadata errors never count as definitive deletion. New records include the SVG/size revision and a resource-fork fingerprint. Changed artwork reapplies only while the recorded payload still matches; manual replacements are left alone. Failed, manual, legacy-ambiguous and interrupted-ambiguous removals retain their records. Cleanup stops the service first and removes state/cache only after all ownership cleanup succeeds.

Watches register before startup reconciliation. Shared exclusions suppress Finder metadata, opt-outs, symlinks and dependency/build contents. Dirty paths coalesce overlapping subtrees and repeated events. The callback uses a nonblocking queue capped at 256 events, rejects events larger than 64 paths into missed-event recovery, and bounds the dirty set at 2,048 paths and 256 paired moves. Debounce is 500ms of quiet with a fixed five-second deadline. File saves check that object; creates and directory moves reconcile affected subtrees. Startup, backend errors, rescan notifications and overflow conservatively reconcile configured roots. Pruning uses journal ranges for affected paths/subtrees rather than checking every global record on each tick. Repository discovery stays within canonical, deduplicated watch roots and consistently lets parent repos cover nested repos/submodules.

Optional selection is available in config and on `preview`, `apply` and `reconcile`: folders-only, specific matches only, literal name/path exclusions, and Git ignore rules. All space-saving policies remain disabled by default. Specificity follows the matching rule, not the icon ID. Git evaluates tracked-file exceptions, negations, global ignore rules and worktree/nested-repository rules. The Git listing is capped at 16 MiB and fails closed if evaluation fails. Changing selection does not silently remove existing icons: preview lists excluded records and `reconcile` explicitly removes only verified owned icons. No estimated physical disk saving is promised.

AppKit reuses up to 64 retained images on its calling thread and retains per-call autorelease pools. Embedded SVG bytes are borrowed; bounded prepared paths are reused across repos/ticks. Disk SVGs are reread so resource edits invalidate their content revision. Candidate metadata/identity is reused; normal apply avoids preview sorting and duplicate candidate maps. Typical resource forks are fingerprinted in one syscall with a stack buffer; large forks have a bounded fallback. Minimal Objective-C and vector-renderer dependency features are enabled by default. `extended-svg` restores text/font, raster-image and compressed-SVG support for disk themes; text loads system fonts once and has a separate cache revision.

The archive contains only reachable artwork plus the MIT notice; `tools/trim_theme.py` repacks it reproducibly without changing SVG bytes. The disposable PNG cache is trimmed to 1,024 files / 64 MiB after application and on watcher startup, and every 64 new renders during sweeps. Only content-addressed PNGs are eligible; user files, symlinks, journals and backups are preserved. The watcher bounds its active log and one tail backup to approximately 2 MiB each, truncating the active inode so launchd's open descriptor remains valid. Startup bounds logs before config parsing, including crash-loop errors. Active sweeps can temporarily exceed cache/log targets; idle log checks happen every 30 seconds.

## Measurements

Fresh release builds of the baseline and implementation were run against temporary fixtures. JSON save counters were added **only to temporary source copies**, then discarded. They count serializations/bytes passed to saves, not physical SSD writes. Three fresh 1,000-file fixtures per version were used, with three no-op repeats per fixture. [Exact run data and method](performance-and-storage-benchmarks.json) are saved alongside this report.

| Check | Before | After | Interpretation |
| --- | ---: | ---: | --- |
| No-op apply, 1,002 items | 1 save; 144,268 bytes | 0 saves; 0 bytes | Eliminated unchanged journal writes |
| Undo after replacement fixture | 1,003 saves; 72,276,574 bytes | 5 saves; 419,302 bytes | 99.4% fewer serialized bytes; new version also undoes the repaired replacement |
| Undo median wall time | 0.456s | 0.310s | Approximately 32% lower in this small sample |
| Initial apply median | 10.274s | 11.216s | No demonstrated initial-throughput gain; ranges overlap and AppKit remains dominant |
| Initial apply serialized bytes | 509,816 | 1,390,843 | Higher due to durable intent + fingerprint checkpoints and larger records |
| Full no-op apply median | 0.040s | 0.061s | Extra payload verification has a measurable cost |
| Empty apply with 105,400 synthetic entries | 1 save; about 13.3 MB | 0 saves; 0 bytes | External CLI still parses/indexes state; watcher reuses live state |
| Empty large-journal apply median | 0.063s | 0.073s | Small parse/index overhead; eliminated write is the meaningful gain |
| Excluded watcher build (500 writes) | 2 saves; 13,930 bytes | 0 saves | No icon/journal work for excluded contents |
| Watcher content edit | 2 saves; 13,930 bytes | 0 saves | Unchanged icon content leaves ownership journal alone |
| New file during continuous events | Not applied within 8.04s | Applied within 5.11s | Demonstrates progress without a quiet window |
| Scanner-only work, 100 ordinary-save checks | 100,200 candidates; 83.46ms | 100 candidates; 1.71ms | 1,002 → 1 candidate/check; about 49× faster traversal probe |
| Embedded archive | 527,762 bytes | 309,027 bytes | 41.4% smaller, including the MIT notice |
| Release binary | 4,627,904 bytes | 2,710,144 bytes | 41.4% smaller |

The new full scanner alone took 270.03ms for 100 complete passes versus 83.46ms previously: it now gathers reusable identity metadata. This is not a full-apply comparison. Normal saves avoid that full scan. The fixture journal grew from 144,268 to 285,549 bytes because fingerprints and revisions strengthen ownership protection. Reusing NSImages does not demonstrate an overall initial-apply speedup here; no claim is made for it.

Timings are small samples on this Mac with warm caches and uncontrolled background activity. Hard counts, archive sizes and PNG equality are stronger evidence than elapsed-time differences. The baseline and new undo operate on 1,001 and 1,002 actual icons respectively because only the new version repairs the atomic replacement.

## Verification

- 25 unit tests (full suite plus targeted checks after final fixes), including all shipped-resource 256px renders, shared exclusions, coalescing/overflow, backend rescan recovery, journal migration/fail-closed behaviour and cache bounds.
- 17 CLI/macOS integration tests: isolated real icon application/undo, no-op journal identity, atomic saves, moves, manual replacements, ambiguous legacy/interrupted records, failed cleanup retention, locking, read-only preview, explicit policy reconciliation, Git ignore exceptions/worktrees/nested repos, watcher churn/discovery, and log bounds before config errors.
- `cargo clippy --offline --all-targets -- -D warnings`, formatting and release build passed.
- `extended-svg` builds, and its targeted disk-text test verifies nontransparent text output. The default build explicitly rejects that text resource with a rebuild hint.
- Separate baseline/new probes compared all 857 rendered PNG outputs: identical bytes and 256px widths.
- Temporary benchmarks exercised excluded-event churn and continuous-event progress; bounded overflow/rescan paths were exercised by unit tests. No claim is made for a measured maximum RSS under a real sustained event storm.

The tests and benchmark repositories, state, HOME, global Git config, XDG directories, rendering caches and build copies were isolated under temporary directories. Temporary processes were stopped, metadata fixtures undone where applicable, and disposable benchmark/build copies removed after preserving results. The installed service was never stopped or restarted. Local source, release build and project test artifacts remain available for development.

## Persistence decision and limitations

Keep the compatible JSON journal for now. Measurements confirm that no-op and undo write amplification was the first problem to fix, and those saves are removed/batched. Small initial applies are still dominated by AppKit. There is no measured initial-throughput gain to justify adding a database or journal migration in this change. Genuine changes still save the global snapshot twice per apply batch; a large initial sweep or many changes against a large inventory can remain expensive. Those workloads should be measured before choosing sharded snapshots or transactional storage.

Legacy entries have no trustworthy payload fingerprint. Custom icons on those objects, and new icons left in the narrow interruption window before the fingerprint checkpoint, remain ambiguous. They are preserved and reported as retained on undo/reconcile/clean; this means automatic cleanup cannot reclaim all legacy installation metadata while also guaranteeing preservation of manual replacements. Unknown/corrupt journals and backups are never abandoned. The implemented selection policies have not been applied to the existing installation.

A move outside watched/selected scopes, a cross-volume move that changes identity, or loss of notifications can exceed path/identity recovery; icons are preserved rather than overwritten, and recovery only searches configured/selected scopes. There is a small unavoidable filesystem race between checking identity/payload and path-based AppKit calls; simultaneous manual changes in that window cannot be fully serialized by the application's own advisory lock. Extended-theme configuration/association edits and global Git exclude configuration changes require a watcher restart; ignore changes outside watched roots are evaluated on the next relevant scan. No guarantees were established for remote volumes, physical APFS reclaimability, every Finder zoom level or a full workday's CPU/RSS. No direct resource-fork replacement, smaller raster size or alternative ICNS format was introduced.
