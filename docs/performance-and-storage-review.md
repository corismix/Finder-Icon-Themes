# Performance and storage review

14 October 2026. Reviewed the complete Rust source, Cargo configuration, installer, README and existing tests at commit `a89c81d`, plus the running installation. No application code, configuration, service registration or existing icons were changed. Experiments applied icons only to temporary fixtures outside the configured watch root.

The strongest recommendation is to make updates incremental and stop writing unchanged state. Keep the current 256px rendering. For maximum space savings, make file icons optional and support selective theming: custom icon data attached to individual items is overwhelmingly the largest footprint.

## Measured baseline

The live service was running with `files = true`, `folders = true` and `/Users/casey/Code` as its watch root. Counts changed slightly during measurement because the service remained active.

| Component | Observed footprint |
| --- | ---: |
| Managed items | Approximately 105,400: 93,200 files and 12,200 folders |
| Resource forks on managed file paths | 841,915,072 bytes / 803 MiB |
| Resource forks in managed folders' `Icon\r` files | 85,143,776 bytes / 81 MiB |
| Total measured icon resource-fork payload | 927,058,848 bytes / 884 MiB |
| State journal | Approximately 19.18 MB / 18.3 MiB |
| Render cache | 466 PNGs; `du` reports 2.3 MiB |
| Installed binary | 4,627,904 bytes / 4.4 MiB |
| Embedded theme archive | Approximately 515 KiB compressed |
| Entire checkout, including Git history | 1.9 MiB before this report |
| Watch log | Empty at measurement |

Resource-fork totals were obtained using size-only `getxattr` calls across a journal snapshot, without reading the payload of every icon. 33 paths lacked the attribute or had disappeared. These are **logical payload bytes**, not an exact measurement of exclusive APFS allocation or immediately reclaimable space; block rounding, clones and snapshots affect that. Existing manual replacements on recorded paths also cannot be distinguished from app-owned payload by the current journal.

One process snapshot showed a 68% CPU burst. Subsequent snapshots showed 0%; a three-second stack sample showed the watcher waiting. That sample reported a 117.1 MiB physical footprint and 1.1 GiB historical peak. The peak's cause is unproven; the unbounded event queue is a plausible contributor, not a demonstrated explanation.

### Direct reproductions against a fresh release build

| Temporary experiment | Result |
| --- | --- |
| First apply to 1,000 TypeScript files and two folders | 1,002 icons applied; 10.57 seconds |
| Apply again to the same fixture | 0.046 seconds; no icons changed, but the journal was replaced with identical contents |
| Write inside excluded `node_modules` while watching | Journal replaced with identical contents after debounce |
| Atomically replace one themed file, then apply | Replacement skipped as stale; 1,001 other icons unchanged |
| Undo that fixture | 1,001 icons removed; 0.46 seconds; replaced file's old record forgotten |
| Apply to an empty directory using a private copy of the live journal | 0.080 seconds; approximately 19.18 MB rewritten despite zero candidates |
| File block allocation for the 1,000-file fixture | Increased by 8,192,000 bytes after applying icons |

Times are single-run observations on this Mac, not portable benchmarks. The large-journal experiment only touched its private copy; it did not apply or remove icons at the recorded live paths.

## Findings and specific fixes, in priority order

### 1. Every event can scan an entire repository — confirmed, highest runtime payoff

Locations: [watch.rs:40](/Users/casey/Code/apps/Finder-Icon-Themes/src/watch.rs:40), [watch.rs:74](/Users/casey/Code/apps/Finder-Icon-Themes/src/watch.rs:74), [apply.rs:65](/Users/casey/Code/apps/Finder-Icon-Themes/src/apply.rs:65).

The callback discards event type and attributes, retaining only paths. Events from `.git`, build/dependency directories, Finder metadata and ordinary content writes all map back to a repo, then run `collect_candidates` across that repo. Scanner exclusions do not filter watcher events. The `node_modules` fixture reproduced this. During a sync, the app's own icon writes can enqueue more events and cause another scan; the unmodified follow-up generally stops after another pass rather than creating an unconditional infinite loop.

**Fix:** retain full events and share a path policy between watcher, discovery and scanner. Reject excluded descendants, `Icon\r`, `.DS_Store` and AppleDouble paths early. Process creates, removals, renames and file replacements at the affected paths/subtrees. A content change to a tracked path usually only needs an identity check: artwork depends on its name, not its contents. Keep marker creation/removal as a subtree reconciliation event. Handle directory events and unknown/rescan events conservatively rather than filtering every metadata event indiscriminately. This preserves recovery when icons are externally cleared or saves replace files.

Maintain a small repo index so event resolution does not repeatedly walk up the filesystem. Treat `.git` pointer/directory creation and removal as discovery changes. Register watches **before** startup reconciliation; currently changes during the initial sweep occur before watches exist. Check roots are canonical, deduplicated and contain each affected target; `repo_for` currently climbs beyond them. Collapse nested/overlapping roots and reconcile a parent repo/submodule policy consistently.

Notify exposes both event kinds and missed-event information; see [notify 8.2.0 Event documentation](https://docs.rs/notify/8.2.0/notify/struct.Event.html). Errors and `need_rescan()` are currently lost and should trigger a bounded reconciliation.

### 2. Journal rewrites amplify disk traffic — confirmed, highest write reduction

Locations: [apply.rs:130](/Users/casey/Code/apps/Finder-Icon-Themes/src/apply.rs:130), [apply.rs:144](/Users/casey/Code/apps/Finder-Icon-Themes/src/apply.rs:144), [apply.rs:182](/Users/casey/Code/apps/Finder-Icon-Themes/src/apply.rs:182), [state.rs:136](/Users/casey/Code/apps/Finder-Icon-Themes/src/state.rs:136).

Every save serializes and atomically replaces the entire global journal. Apply saves even with no changes; watcher prune saves again. One unchanged repo event therefore writes about **38.4 MB** at the current journal size. A tick affecting R repos writes at least `(R + 1) × journal_size`, even with no icon changes. Startup has the same per-repo final saves.

Apply batches at 256 items, but undo saves after **every single item**. Using the current count and average entry size, applying a similarly sized inventory from empty would serialize approximately **4 GB cumulatively**, and undoing all of it approximately **1 TB cumulatively**. These are growth/shrink models of bytes passed to filesystem writes, not measured physical SSD writes or final disk occupancy. Paths and repo distribution change the totals.

**Fix first:** track whether state changed; save nothing for a no-op. Batch undo checkpoints, keeping failed removals recorded. Preserve journal-before-icon ordering for apply; deferring all writes until the end would lose ownership on interruption. Keep live state in memory under proper mutation locking, instead of rereading/parsing it for every event.

**Then, if large initial sweeps remain costly:** use per-repository snapshots or a transactional store so each update persists only changed records. An append log also works but needs partial-record recovery and compaction. Do not compress and rewrite the same global blob as the primary fix: that still scales with all managed entries and adds CPU work. Relative paths, parent-prefix factoring, numeric identities and interned icon IDs can reduce state size and memory after the write frequency is fixed. Preserve/migrate the old schema rather than abandoning its ownership records.

### 3. Theme fewer items to reclaim substantial space — confirmed trade-off

Locations: [matcher.rs:57](/Users/casey/Code/apps/Finder-Icon-Themes/src/matcher.rs:57), [scanner.rs:71](/Users/casey/Code/apps/Finder-Icon-Themes/src/scanner.rs:71), [config.rs:11](/Users/casey/Code/apps/Finder-Icon-Themes/src/config.rs:11).

All files and folders receive custom artwork, including generic fallbacks. Cached artwork is shared during preparation, but macOS stores icon metadata on each target. Deleting the PNG cache does not remove those copies.

- **Folders only:** removes the need for approximately 803 MiB of measured file payload, about **91%** of the total, while preserving folder icons. It also sharply reduces initial AppKit calls and journal entries. It changes the visible product: files return to their normal icons.
- **Specific matches only:** the generic `file` and `folder` payloads measured approximately **214 MiB combined**, around **24%** of total icon payload. Add an explicit fallback toggle; do not infer specificity solely from an icon ID.
- **Configurable subtree/repository exclusions and Git ignore support:** prevent generated assets, downloaded material, caches and arbitrary build-directory names from becoming managed. The hardcoded list does not consult Git ignore rules. Preserve tracked files even when a pattern ignores them, and handle submodules, worktrees and new untracked source files deliberately.
- **Optional per-repository budget:** preview estimated added metadata before a large apply; allow explicit full theming when wanted. The 120,000-entry scanner limit bounds entry count, not icon storage.

**Changing `files = false`, adding a marker or narrowing roots does not remove existing icons.** Current code only stops selecting them. Provide an explicit reconciliation/cleanup operation that removes previously owned icons excluded by the new policy, preserves manual customizations, and retains failed removals for retry. Otherwise the promised disk savings never materialize for this installation.

For maximum savings without losing all customization, my pick is folders-only plus specific folder matches where desired. If file artwork is essential, retain file icons but add exclusions and optional generic fallbacks.

### 4. Queue growth and debounce can become unbounded — confirmed mechanism; peak cause unproven

Locations: [watch.rs:39](/Users/casey/Code/apps/Finder-Icon-Themes/src/watch.rs:39), [watch.rs:50](/Users/casey/Code/apps/Finder-Icon-Themes/src/watch.rs:50).

The channel and `changed` vector have no bounds. Duplicate paths accumulate until there is a full two-second quiet window. Continuous builds can indefinitely postpone applying new icons and grow memory; events also queue while AppKit is busy.

**Fix:** coalesce into a bounded path/subtree set, collapse children when a parent is already dirty, and enforce a maximum debounce deadline as well as a quiet window. On overflow, mark the affected repo/root for reconciliation instead of retaining every event or silently dropping it. Keep the callback nonblocking. Limit work per turn so other repos are not starved by a large one. A low CPU/I/O scheduling priority or pause mechanism can reduce disruption but does not replace eliminating unnecessary work.

### 5. Global pruning and duplicate metadata checks make every tick expensive — confirmed

Locations: [watch.rs:96](/Users/casey/Code/apps/Finder-Icon-Themes/src/watch.rs:96), [apply.rs:70](/Users/casey/Code/apps/Finder-Icon-Themes/src/apply.rs:70), [apply.rs:73](/Users/casey/Code/apps/Finder-Icon-Themes/src/apply.rs:73), [scanner.rs:50](/Users/casey/Code/apps/Finder-Icon-Themes/src/scanner.rs:50).

Every sync calls `exists()` on every global entry, including unrelated repos: approximately 105,400 existence checks. Apply also computes `affected` for a report counter, then repeats identity checks in its decision loop. An intermediate map copies all candidate paths just to remember custom-icon booleans. Scanner fetches file type twice, allocates full/relative paths and icon strings for each item, and sorts the whole candidate list.

**Fix:** prune deletions within the affected subtree/repo; do a global reconciliation only at startup or for actual missed-event recovery. Make one candidate-decision pass, reusing metadata and checking custom-icon ownership there. Eliminate the intermediate map and unused duplicate calculation. Avoid sorting applies when only preview output needs deterministic order. Intern icon sources/IDs and stream or batch candidates if large-tree peak memory is still significant. In folders-only mode, reject ordinary files before icon matching. These are secondary to incremental event handling but straightforward wins.

### 6. First apply repeatedly loads the same image through AppKit — confirmed; speedup unmeasured

Location: [macos.rs:28](/Users/casey/Code/apps/Finder-Icon-Themes/src/macos.rs:28).

`png_cache` avoids repeated SVG rendering within an apply, but `set_icon` constructs and loads a new `NSImage` from the PNG for every target. The 1,000-file fixture took 10.57 seconds to apply versus 0.046 seconds to check unchanged items. That establishes the cost of initial application, not how much image reuse alone would recover.

**Fix:** reuse a bounded cache of retained `NSImage` objects on the AppKit calling thread, with per-call autorelease pools retained. Cache prepared PNG paths across repos/ticks using an explicit theme fingerprint. Embedded SVG access can borrow bytes instead of cloning them; disk override roots can be canonicalized once, with resource updates invalidating caches. Avoid unbounded parallel AppKit writes: they may add contention and complicate metadata and journal ownership.

Prepared resource-fork payloads written directly could potentially bypass repeated AppKit conversion, but Finder refresh, existing FinderInfo/resource preservation, permissions and undo need a separate prototype before adopting this. It is an experiment, not an established safe replacement.

### 7. Ownership handling can leave unreclaimable or skipped icons — confirmed

Locations: [apply.rs:83](/Users/casey/Code/apps/Finder-Icon-Themes/src/apply.rs:83), [apply.rs:203](/Users/casey/Code/apps/Finder-Icon-Themes/src/apply.rs:203), [apply.rs:235](/Users/casey/Code/apps/Finder-Icon-Themes/src/apply.rs:235), [state.rs:92](/Users/casey/Code/apps/Finder-Icon-Themes/src/state.rs:92).

An atomic-save replacement is skipped forever while its stale record remains, reproduced in the fixture. A rename can carry an icon with the object, while its old-path record is pruned and its new path is treated as somebody else's custom icon. Schema changes move the entire old journal aside and start fresh, abandoning active ownership. `clean` clears records even when icon removal fails and does not check identity as undo does.

**Fix:** forget a stale replacement record, inspect the new object's existing custom icon, and theme it when uncustomized. Reconcile moves by stable identity rather than just path. Migrate schemas and preserve failed removals. Before reclaiming excluded items, verify identity and ownership; a stored icon fingerprint can detect a manual replacement on the same inode. State currently compares only icon ID, so an SVG/theme revision with the same ID is not applied. Add a source/theme revision fingerprint and reapply changed artwork deliberately.

### 8. Mutations are not consistently locked — confirmed prerequisite for safe optimization

Locations: [main.rs:146](/Users/casey/Code/apps/Finder-Icon-Themes/src/main.rs:146), [main.rs:241](/Users/casey/Code/apps/Finder-Icon-Themes/src/main.rs:241), [watch.rs:61](/Users/casey/Code/apps/Finder-Icon-Themes/src/watch.rs:61).

Apply/undo load state before acquiring their lock. Watch and clean do not acquire it at all. Concurrent commands can overwrite ownership records. Clean removes icons before stopping the service, so the watcher can reapply during cleanup. A crashed command also leaves a lock file with no stale-lock recovery.

**Fix:** centralize a process lock, acquire before reading/mutating state, and share it across CLI and daemon writes. An OS advisory lock releases on process exit. Coordinate daemon stop before cleanup. If retaining state in daemon memory, reload after another process's generation changes. This prevents performance work from introducing stale-state overwrites and lost undo information.

## Smaller savings and choices to keep

- **Keep 256px.** A representative AppKit probe at 64/128/256/512px produced resource forks of approximately 163/276/30/99 KB. Reducing resolution increased storage dramatically for that artwork. Excluding QuickDraw elements did not change the 256px payload. These values are artwork-specific; the smaller actual deployed icons in the baseline are the relevant installation measurements. [Apple's custom-icon API](https://developer.apple.com/documentation/appkit/nsworkspace/seticon(_:forfile:options:)) is the existing storage path.
- **Single-representation ICNS is experimental.** One deployed generic file fork contained two 3,172-byte representations (`ic13`, `ic08`). A minimal single-representation resource might reduce payload, but Finder rendering across scales and macOS versions was not tested. Do not promise deduplication from caching PNGs, or replace all `Icon\r` files with shared hardlinks: ownership and independent updates need to remain safe.
- **Trim build dependencies.** `resvg` defaults include text/font and raster-image support. None of the 857 SVGs reachable by this matcher use text, embedded images or foreign objects. Disable unnecessary default features, preserving any intended disk-theme capabilities. Objective-C binding defaults enable far more AppKit/Foundation APIs than the explicitly requested `NSWorkspace`, `NSImage` and `NSString`; use minimal features and required `std`/dependency features. Validate by rendering the shipped icons and running integration tests. Binary/build savings were not benchmarked.
- **Trim embedded artwork.** Archive inspection found 1,251 real SVGs, of which 394 are unreachable by the current matcher, plus 204,402 bytes of uncompressed AppleDouble metadata. The loader mistakenly retains AppleDouble entries ending in `.svg`. Repack without AppleDouble and unreachable resources, preserving attribution/licensing. This saves startup parsing/allocation and some archive space, but is tiny beside per-item metadata. Pre-rendering all artwork could remove the SVG renderer but may enlarge bundled assets; measure before choosing that trade-off.
- **Bound disposable storage.** PNGs are content-addressed with no eviction, so theme updates accumulate old variants. Add cache pruning and make full cleanup remove the cache after all owned icons are successfully dealt with. Logs have no rotation; repeated errors or LaunchAgent restart loops can grow them. The present log is empty, so this is prevention rather than a current saving. Preserve journal backups until their ownership is migrated/reconciled.
- **Avoid repeated Git setup.** Resolve the global exclude path once per mutation session and skip `git config` when the recorded rule is valid. `ensure_global_rule` currently sets `global_owned = false` when it encounters its previously added rule, causing subsequent applies to lose removal ownership; preserve ownership when the recorded file/rule matches.
- **Build settings already help.** Release LTO and stripping are enabled, and setup removes the build tree. My combined debug/release audit build occupied 1.7 GiB temporarily and was removed after verification. Keep build caching optional: removing it saves retained disk but makes subsequent builds slower. `opt-level = "s"/"z"`, `codegen-units = 1` or prebuilt binaries warrant measurement rather than blanket changes; tiny binary savings do not justify slowing the main runtime.
- **Keep the useful existing protections.** Autorelease pools, symlink avoidance, excluded dependency contents, 256-item apply batches and atomic journal replacement all serve real purposes. Improve them rather than replacing them with a wholesale architecture rewrite.

## Recommended implementation sequence

1. Preserve the current visual result: dirty-state saves, batched undo, shared locking, filtered/coalesced event queues, bounded debounce and scoped pruning. Add direct regression checks for no-op writes, excluded-event churn, atomic replacement and queue overflow/rescan recovery.
2. Make normal updates incremental by path/subtree, with startup/missed-event reconciliation. Ensure a content-only edit does work proportional to that path, and an excluded build does no icon/journal work.
3. Add explicit selective-theming policies and safe reconciliation of already installed icons. Folders-only is the biggest space trade-off; generic-fallback removal is a smaller compromise.
4. Reuse AppKit images, slim feature flags and trim the archive/cache. Revisit the persistence format if large sweeps still cause significant write traffic; prototype minimal ICNS only if further payload savings justify it.

The desired observable results are: idle blocking with no journal writes; excluded events producing no scans/writes; ordinary saves touching only affected objects; bounded memory during continuous builds; interruption-safe ownership; and disk usage proportional to the chosen icon policy.

## Verification and remaining gaps

`cargo test --offline` passed all **18 unit tests and 9 integration tests** on macOS. A fresh release build succeeded. Additional temporary probes established no-op journal writes, excluded-build event handling, atomic-replacement skipping, initial apply/undo timing and size-dependent AppKit storage. Live state and metadata were inspected read-only. Temporary build and icon fixtures were removed.

No optimization has been implemented. No controlled measurement was made of steady-state CPU over a real workday, exact physical APFS reclaimability, Finder's rendering quality at every zoom, image-cache speedup, dependency-feature binary savings, direct resource-fork writes, remote volumes or sustained event storms. These gaps affect numerical promises and experimental options, not the confirmed repeated scans, global writes and per-item storage footprint.
