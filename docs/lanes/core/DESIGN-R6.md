# R6: an ordered scene with compositional summaries and explicit change evidence

State: proposal (`LEAD`), awaiting lead review. No representation change is implemented or accepted.
Baseline: `aa9943e7ae50595f6af026773ee82b46a4b85ae0`. The frozen measurement card and analytical
cost model are [here](cards/2026-11-11-cost-model.md) and [here](COST-R6.md). The matrix harness
compiled, but no R6 instruction measurements ran before the user requested handoff. See
[HANDOFF](HANDOFF.md) for exact completion state and continuation traps.

My recommendation is a **wide sequence tree over the existing packed leaves**, with rank, depth,
spatial and change summaries maintained by the same split/join operations. Mutation should produce
explicit evidence of changed support and, where proved, unchanged transported content. Repaint tiles
are a consumer of that evidence, not its semantic authority. Keep the UI model exclusively borrowed.
Publish immutable scene resources at an explicit boundary; do not introduce concurrent mutation of
live pages.

This is a direction, not a claim that a B-tree automatically wins. Its dependent loads and summary
maintenance must earn their cost at 1,000 records. Neither an asymptotic argument nor a large-N win
waives the 1.02 limit. An ordered hierarchy also cannot promise sparse spatial work when every
paint-order interval overlaps the whole window.

## What the six observations get right, and what changes the diagnosis

| Observation | Finding and consequence |
|---|---|
| 1. Hard domains | Correct for the 4,096-pixel damage window and carved columns. Worse for coordinates: public `RecordInit`, absolute summaries and clips are already `NRect` with clamped i16 coordinates. Page offsets do **not** establish an unbounded world domain. The stored coordinate range is about ±32,766, separately from the damage limit. Heap-owned `Col`s can grow; carved GPU-visible columns cannot. Clip IDs are u16, with permanent allocation/deduplication rather than a complete reclamation protocol. Compact local coordinates and fixed leaf capacity are useful; fixed window size, silent narrowing and construction-time resource caps are not necessary consequences. |
| 2. Flat directory | Correct, with two corrections. There are **five** parallel directory columns, plus prefixes/chunks. Default `BlockedPrefix::invalidate_from` ignores its starting index and rebuilds from zero: even a tail split pays O(P). There already is a spatial chunk level (`CHUNK=64`), contrary to the older DESIGN deferral. It has no recursive rank/depth summary. Stable generational handles also already exist; positions are ephemeral, not the only identity API. |
| 3. Repeated insertion at mount | Mostly reject the premise. `insert_run` can build many full pages in one call. `region_mount` batches entering rows, and `lr_fill` batches each contiguous missing run, including an ordinary initial mount. Fragmented holes cause separate inserts. A keyed move is more concerning: `lw_remap` can materialize and `rewrite_run` the whole retained window. Mount also allocates instances, glyphs, spaces, handles and layout nodes; it cannot literally be only a memcpy. Parallel core filling is not the first missing mechanism. |
| 4. General motion | Accept as a representation goal, reject unconditional copying. A structural splice does not by itself certify that the composite pixels below it translated: layout, clipping, overlap, transparency, backgrounds and effects matter. A reorder with unchanged positions changes paint order, not a motion vector. Copy costs bandwidth and may lose to a cheap redraw. Preserve bitplanes and add proven transport, with a cost policy downstream of correctness. |
| 5. Readers / COW | `freeze_into` already copies only page bodies whose version/write generation changed. It still scans all pages and clones directory/prefix/clip/ring data. A dirty publication mechanism can remove that scan without universal COW. A scene snapshot must also retain glyphs, instances, spaces, clips and image resources. bbtree optimistic single-entry reads do not provide an atomic scene epoch. |
| 6. Parent routing | Accept the gap, replace depth alone as its admission criterion. A root with a million direct children has depth one and a bad backward scan. Repeated chains also scan across earlier chains to find their common root. A recursive minimum-depth summary makes these searches logarithmic without a parent column. A reader-requested parent projection can then serve repeated walks, but its rebuild must be charged, and stable IDs—not shifted positions—must be its values. |

Other costs remain above the core: `List::order` is a Vec, `HeightIndex::assign_from` rebuilds a suffix,
`lr_prefix` sums prior mounted rows, and carrying a space walks descendant spaces. A logarithmic core
under these operations does not make a keyed-table turn logarithmic. The same sequence measure
(count, extent) should eventually replace those independent positional suffix indexes. That is a
coordinated façade change, not an excuse to modify another lane now.

## Correctness witnesses before performance work

Three standalone probes on the baseline produced the following results. The reproducible source is
[the witness](witness-r6.rs); it uses ordinary core interfaces, no test-only hooks. Existing core tests
all passed despite these cases. These findings are local witnesses, not an independent replay or an
exhaustive transform audit.

1. Window 320²; record at `[100,100,120,120]`; save the hit cell at `(110,110)`. Scroll the record down
   64 pixels by copy through the whole window. `hit` changes `Some(1) -> None`, but `cell_valid` is
   **true**. Repaint strips do not invalidate semantic hit support. The façade calls `cell_valid` to
   skip hover-chain recomputation; callers may invalidate in other circumstances, but the core
   certificate itself is false.
2. Record at `[10,10,20,20]`, affine translation +100 in x. Its conservative ink becomes
   `[109,9,121,21]`, `hit(115,15)` returns it, and culling `[110,10,120,20]` returns **nothing**.
   Directory bounds include the transform; the group/record cull only adjusts for the translation
   offset. The affine and compact spatial representations disagree.
3. Remove that transformed record and insert an ordinary record into the empty tree. The reused
   page retains `xm/xv`, so the new record still appears at `[109,9,121,21]`. `PagePool::free` resets
   sentinels, group ink and offset but not the affine fields.

These are reasons to separate semantic invalidation from repaint, and to make each summary's frame
explicit. They are not permission to weaken any existing exact tile or pixel gate. First settle a
small correctness card that captures these witnesses and fixes their contracts; do not combine its
acceptance with a tree rewrite.

## First-hand bbtree lessons

Read directly from the reference checkout: design 06 (cost model), 07 (node plans), 09 (concurrency,
including its race caveat), 12 (batch API), and decisions D25/D45. Reference provenance belongs in
[COST-R6](COST-R6.md); paths here are checkout-relative descriptions, not private machine paths.

- Count chained loads separately from streamed lines. Two short binary searches in today's prefix
  already form chains; replacing them with a wide-node scan may help. A new hierarchy adds child
  address dependencies. Saving a warm level is not necessarily saving a DRAM miss.
- Charge a SIMD node at **capacity**, not average fill. Doubling fanout lost in bbtree despite fewer
  levels. Start with fanout 32; compare 16/32/64 under a separate card, preserving all small cells.
- Bulk construction and split/join belong to the same structure. bbtree D25 cuts along the boundary
  path instead of moving every element. That transfers more directly than its key-prefix machinery:
  bough has ranks, depth thresholds and rectangles, not sorted string keys.
- A retained end cursor may save descent, but must be invalidated by the actual structural operation.
  It does not solve today's full prefix rebuild on a split. Measure fit, split and stream separately.
- Node plans centralize which columns move together. bough should give split/join/retire one place
  to maintain every summary and generation; adding an optimization should not add another loop to
  every mutation method.
- bbtree's shared map has single-entry semantics, latches for closures, and formal racing plain
  reads explicitly acknowledged in design 09. It is not a ready-made safe Rust snapshot protocol.
  Its volatile reads and version checks are not adopted here.
- Interleaved lookup lanes hide independent cold misses; they do not remove CPU work. No prefetch
  coroutine executor is proposed for the single pointer hit path. Parallel immutable traversals can
  first partition child ranges with known ownership and measure useful work per task.

## A concrete sequence representation

The first structural experiment should retain the current 64-record leaf and its GPU ABI. Replace
flat directory rank/depth maintenance with a recursive ordered summary node. An initial layout,
with eight-byte child references (index/generation or equivalent stable arena reference):

```rust,ignore
#[repr(C, align(128))]
struct Branch32 {
    header: Header64,             // len, height, aggregate count/min, generation; exactly 64 B
    end: [u64; 32],                // inclusive child prefix ends: 256 B
    min_depth: [u16; 32],          // 64 B, PAD_DEPTH in unused slots
    ink: [NRect; 32],              // 256 B, same numeric contract as current first experiment
    child: [ChildRef; 32],         // 256 B; total 896 B = seven hardware lines
}
#[repr(C)]
struct Header64 {
    records: u64, generation: u64, parent: ChildRef, // 24 B
    origin: [i64; 2],                               // 16 B
    depth_bias: i32, min_depth: u16, len: u8, height: u8, // 8 B
    flags: u32, reserved: [u8; 12],                  // 16 B
}
// Root is a tagged leaf or Branch32. No empty levels. Leaves retain stable identity.
struct Cursor { leaf: LeafKey, slot: u16, topology_generation: u64 }
```

This is a proposed layout, not compiled size evidence. Widening depth or ink changes the layout and
needs its own measurement. At P=16/157/1,563/15,625 leaves, a full fanout-32 tree needs approximately
1/2/3/3 branch levels (fill can increase this). Header and rank columns occupy three 128-byte lines;
min-depth one; rectangles two; child references two, with shared boundary lines. Do not fetch all
seven lines for a rank-only query. An arena-index child may need a mapping load; a direct stable
pointer avoids that load but must preserve page lifetime. Measure the actual chosen representation.

Rank descent compares 32 prefix ends with the query, counts the ends <= rank, subtracts the previous
end and follows one child. NEON u64 compares are two lanes, so this is sixteen comparisons per full
node, independent loads within a level. A u32 count specialization is not assumed free; it needs a
proven subtree bound or is rejected. Min-depth descent scans 8 u16 lanes at a time, chooses the last
child before the cursor whose minimum is below the target (parent), or the first later child whose
minimum is <= target (subtree end). Rectangle scans preserve the current negated-max `lt8` shape,
then enumerate child masks in painter order. Narrow leaves still scan groups of eight and use the
existing compact-eight output. Batch cursors amortize rank descent across a range.

**Invariant:** every child summary is an exact rank/depth aggregate and a conservative spatial union
in its declared frame; concatenating leaves yields the semantic paint preorder; handles retain
identity through relocation. Root-to-leaf ranks and cursor generations cannot silently overflow.

Insert/delete split or join boundary paths; bounded-node suffix shifts replace global Vec shifts.
A page changes its local parent/slot on a local rebalance, not every later page's global `dir`.
`pos_of(handle)` sums ancestor rank contributions. An optional caller-owned cursor reuses a path
only while its generation is valid. The same operation maintains size, depth, spatial and changed
summaries. Bulk loading fills leaves, then parents bottom-up; no per-record descent.

A later range-transform stage uses the header's origin/depth bias as lazy inherited tags.
A covered child subtree receives one checked tag update; split boundaries push tags only along
boundary paths. Child bounds/minima are in the parent's local frame, and their parent summary
accounts for the tag. A whole-range translation then visits a logarithmic boundary decomposition,
not every leaf. On reparenting, depth bias adjusts the moved interval without rewriting every
record. Queries and GPU traversal must carry accumulated tags; the present leaf-only shader cannot
consume this for free. The first rank/depth prototype should reserve the layout but leave these
tags zero until their query/renderer contract is separately gated. Future mixed contexts and wide
coordinates must use checked accumulation, not wrapping i16 arithmetic.

**Move is cut/join, not read/remove/insert.** An aligned retained interval transfers interior child
references; only boundary records need copying. The UI must pass a stable run identity and its
transform/parent change. Removing K records still has O(K) semantic destruction if their handles
and resources must be released; moving them need not. Materializing an arbitrary permutation is
also output-sized. Do not compare an O(K)-destruction contract to a lazy-detach claim without charging
retirement later.

Keep a painter-order GPU traversal derived from the same hierarchy. Rebuilding a flat GPU directory
on every structural edit would merely move the suffix cost. A first CPU prototype may expose that
cost as a separate unresolved consumer; it cannot claim the end-to-end win until GPU traversal or
incremental block descriptors remove it.

**Oracle:** scalar preorder Vec, stable handle mapping, exhaustive rank/parent/subtree results after
random split/join/reparent and long churn; retain all current damage/pixel gates. **Mutants:** stale
child count, omitted minimum-depth propagation, dropped boundary record, reused generation, and
reordered cull emission must each fail. **Unused cost:** this is the only sequence representation;
a root leaf has no branch descent. At larger small scenes it still pays branch work. No claim of
zero overhead is made: if any fixed cell exceeds 1.02, revise or reject it.

## Spatial locality and domains

A million records in paint order can have mutually overlapping bounds. A sequence hierarchy cannot
certify disjointness that is absent. The ordered/interleaved matrix is deliberate: it holds record
count and tile damage constant while destroying spatial locality. A 1% tile fraction is not a promise
of 1% record or GPU work. Overlapping transparent content can require every contributing record.

Prefer explicit compositing/clip contexts as scene records with inherited spatial frames. A context
is both a coordinate boundary and a place to prove independent transport or raster work. Physical
leaf boundaries remain independent: one semantic context may span many leaves, and one leaf may
hold several contexts. Do not infer a semantic ancestor from the physical B-tree path. Local 16-bit
ink is a compression format, not the world-coordinate type.

A later wide-domain design uses checked i64 accumulated context origins and a wide rectangle at the
interface. A leaf encodes relative i16 ink only when every needed bound is representable; otherwise
it uses a wide leaf encoding with the **same ordered-tree operations**. Queries subtract the origin
in wide arithmetic before clipping/narrowing. A wide branch bound would be `[i32;4]` (or context-local
conservative bounds under a checked wide origin), making the proposed branch 1,152 B. Arbitrary
f64-affine transforms need conservative outward rounding, nonfinite handling and consistent hit/
cull/raster interpretation; do not use a narrow saturating bound as proof of absence.

This encoding distinction has a real dispatch/storage cost. It is not yet shown free: hoist selection
once per leaf or frame, charge it on compact scenes, and reject a >2% fallback tax. A simpler all-wide
baseline must be compared before accepting compressed complexity. Fixing 5K/6K damage does not require
this whole coordinate redesign: those windows fit existing `NRect`, so tile-domain growth is a
separable first step.

For damage, retain bitplanes in **128-by-128-tile blocks**, indexed by window block coordinates.
Each block reuses today's two-word rows and occupied-y interval; the window owns as many blocks as
its actual extent requires. A 6,016-by-3,392 window has two blocks across, one down; all marks split
at block boundaries before entering the existing kernels. Alternatively compare a dynamically
strided word plane: do not assume blocking wins. The one-block loop must compile as cheaply as the
current entry point, or its cost must fit the fixed gate. Count clearing, OR, shifted carry across
block boundaries, GPU row descriptors and text's direct u128 mark sink together. Changing `Plane`
alone is insufficient.

**Oracles:** independent wide integer rectangles (today's oracle shares the narrow constructor),
windows 4,095/4,096/4,097/5K/6K, negative world coordinates, enormous scrolled offsets, crossing
block edges and clips; exact retained/full redraw at all surface ages. **Mutants:** drop the last
block, truncate an origin, omit a carry, or reuse a stale clip generation. **Unused cost:** bounded
local kernels stay; block routing and wide-format selection are explicitly unqualified costs.
Fixed carved resources grow through new region generations with GPU fences, not reallocating memory
behind live raw pointers. Coordinate memory-lane growth and clip reclamation; do not widen IDs alone
and call the lifetime problem solved.

## Damage as change evidence, with transport as an optional proof

There are three distinct facts:

1. **Semantic support changed:** hit/hover, visibility, order, clips or accessibility may differ.
2. **Stored resources changed:** an epoch consumer needs new page/instance/glyph/space bytes.
3. **Pixels need recomputing:** after valid retention and copies, these tiles remain to rasterize.

They may share a mutation log and summary tree, but are not the same bit. Scroll-by-copy's hit witness
shows why. Style/flag changes with no paint effect still affect semantic readers. An independent
semantic generation can conservatively invalidate a hit cache in O(1) first; spatial support logs
can recover precision later. This first correctness fix need not allocate a second full tile ring.

The writer emits typed effects at actual mutation sites, not by rescanning the scene:

```rust,ignore
struct ChangeBatch {
    epoch: u64,
    changed_pages: PageSet,
    topology: Vec<SpliceOrRelink>,
    support: SupportChanges,       // old/new support and semantic generations
    repaint: TileBlocks,
    transport: Vec<Transport>,    // absent allocation/work when no transport is proposed
}
struct Transport {
    source_epoch: u64,
    context: ContextKey,           // generation checked; not merely a clip id
    destination: RectI32,
    delta: [i32; 2],
    proof: TransportProofKey,
}
```

These are internal contracts. A small list of context-qualified translations is preferable to an
always-present per-tile vector plane. A dense vector plane charges every frame and still cannot say
which translucent contributors moved. The repaint bitplane remains the damage-lab winner; the list
carries positive evidence that some recomputation can be replaced, not arbitrary rectangle damage.
Bound its planning budget; if overlap fragmentation grows too far, repaint the affected union.
That is a bounded-work fallback with identical pixels, not a workload exemption.

**Exactness condition:** for every destination pixel admitted to a copy,
`new_composite(p) == source_epoch_composite(p - delta)` bit for bit. Stable record identity and unchanged
local ink are necessary but insufficient. All composited contributors must be transported together,
or a stationary contribution must be proved translation-invariant there. AA/fringe/filter footprints,
backdrops, clip changes, occlusion reveals, color/scale changes, overlays and paint-order changes are
part of this proof. Unknown content gets INTRA (repaint), never optimistic MOVE. Integer translation
with invariant raster phase is the first admitted transform; scaling/rotation/fractional translation
are not exact copies in general.

`TransportProofKey` must name retained evidence, not a hash or an assertion by the caller. A
concrete first proof kind is an **opaque clipped context translation**:

```rust,ignore
struct OpaqueTranslationProof {
    context: ContextKey,
    content_generation: u64,       // unchanged local paint/order/resource operands
    raster_generation: u64,        // unchanged scale, sampling, color and raster phase
    old_clip: RectI32, new_clip: RectI32,
    opaque_cover: RectI32,         // certified filled background, not merely union ink
    excluded_tiles: TileBlocks,    // overlaps and translated old impressions of outside content
}
```

The context paints an opaque translation-invariant background over its admitted source/destination
interior; all transported descendants keep the same ordered local description and resource versions.
This removes dependence on what lies behind the context. Later painted outside contributors are
queried through scene summaries and excluded at both their new support and their translated old
support; effects enlarge that support by their sampling footprint. Missing opaque coverage or
untracked external/backdrop dependencies rejects the proof. Transparent contexts may later earn an
isolated retained-layer proof with explicit memory/composition costs, but they are not silently
admitted under this one. Content generations must update through the mutation algebra, with an
independent full-render oracle that catches omitted updates. Hash equality alone never certifies
identity. Extending the proof kinds is cheaper than imposing a vector plane on every tile, but each
new kind needs its own falsifying mutant.

For the initial multi-region planner, every copy reads one immutable **previous displayed epoch**
S, held until all GPU reads finish. Destination regions are partitioned into nonoverlapping admitted
tiles. Boundary tiles or tiles with conflicting mappings repaint in full. That deliberately limits
precision to avoid per-pixel modes; it does not claim minimal repaint. If two compatible certificates
cover a tile with the same mapping, merge them. Never read a destination after an earlier copy wrote
it: an immutable source eliminates copy cycles, overlap hazards and order dependence.

For a target surface last holding epoch T, classify each tile as:

- KEEP only with a proof that target T already equals the new epoch;
- COPY only with the equality above for the available source S;
- REPAINT otherwise, including missing/too-old history and ambiguous partial boundaries.

If S is epoch e-1, current-frame certificates suffice for COPY; KEEP outside them still uses the
complete target-age repair history. Do not sum scroll vectors across ages unless composing all
intermediate domains, validity and show-through. Resize can KEEP an unchanged intersection only
when layout/scale/color/compositing are invariant; new exposure repaints. An insert/remove can
produce several context translations after layout settles, but cannot fabricate certificates from
row height alone. Static overlays must repair their original and moved impressions where needed.

Once a copy is proved, choose it only when predicted cost wins. Charge copy reads+writes, submission,
certificate processing, source availability, target age, exposed repair, bin work and glyph density.
One simple regional policy is `copy_setup + bytes/bandwidth + residual_raster < direct_raster`.
Calibration belongs to RT3's backend paired measurements; the historical 0.215ms copy/0.249ms dense
redraw is evidence against a fixed copy policy, not a coefficient to reuse. The scalar model should
choose between already-correct plans; a wrong prediction may lose time but cannot lose pixels.

**Oracles:** full redraw and fresh mount, every ring age including expired history, source equal to
destination, skipped presentations, two scrolls, overlapping/nested clips, row swaps, insert/remove,
stationary translucent overlays, text, resize/reveal, copy cycles and content edits before/after
motion. **Mutants:** delta+1, stale source epoch, treating repaint-clean as semantic-clean, dropping
static show-through, forgetting a boundary tile. **Unused cost:** no vector plane or motion scan on
ordinary paint updates; certificates are emitted by the transform/structural work already occurring.
The empty-log check and any changed-summary maintenance still count against 1.02. An entire-scene
scan to certify a small moving region fails the structural goal even if it passes old scenes.

## Publication and readers

First try **changed-subtree summaries and bounded epoch replay into an exclusively owned replica**.
The hierarchical tree already routes changed nodes; an epoch journal carries relinks and resource
writes. With no change, publishing the same immutable epoch is O(1). With k changed pages it costs
O(k plus changed ancestors plus changed resources), with a full sync when a reader fell behind retained
history. Do not claim O(k) independently of topology height, directory metadata or resource payloads.
The reader owns its replica and never reads memory concurrently written by the UI thread.

COW can later replace replica copying where readers pin many shared unchanged subtrees. Limit it to
immutable **presentation** epochs, not universal application MVCC. First write to a shared page copies
it and the modified ancestor path; shared lifetimes delay reclamation. Writer-only storage can remain
unique, or use a compile-time storage policy selected at the publication boundary. A universal per-write
atomic refcount test is not assumed free. Measure retained bytes across slow readers and the cost of
releasing a large old epoch, not only snapshot acquisition. Publishing a root pointer alone is not a
snapshot if glyph buffers, clip tables or resource handles still mutate underneath it.

**Invariant:** every published root and all transitive resources describe one committed epoch;
release/acquire publication and leases/fences prohibit mutation/reuse while read. **Oracle:** retain
several epochs while editing/recycling pages and all resource kinds, compare each to its scalar saved
scene, stress handoff and run Miri on ownership helpers. **Mutants:** reuse one leased page/resource,
miss one dirty ancestor, publish new topology with old resources, wrap a generation. **Unused cost:**
no reader implies no clone/atomic/retirement queue; existing writer generations can be reused, but
adding a journal branch to all setters must be measured. A zero-tax implementation is an obligation,
not established by choosing the word COW.

Parallel layout still has dependency order (children's sizes, parent's constraints); immutable pages
alone do not parallelize that algorithm. Independent groups can produce version-tagged results and
commit under the unique root borrow. Parallel raster tasks can use disjoint output tiles and pinned
immutable resources. Charge task launch and merge, use sufficiently large existing groups, and keep
small scenes on the same kernels without task scheduling.

## Repeated ancestor consumers

The branch minimum-depth search is the general parent/subtree mechanism. If accessibility or the
inspector repeatedly asks for all ancestors, let that reader explicitly request a parent projection:
`ParentEntry { child: Handle, parent: Handle }`, indexed by stable handle slot and protected by a
full topology generation. Build with one preorder depth stack, O(N) and O(depth) scratch. A retained
reader may replay structural deltas; a one-shot caller pays the build visibly. Query through a typed
projection lease rather than adding an optional-cache branch to every existing `parent()` call.

Depth gating alone misses wide shallow trees. Admission must consider total queries per topology
generation and measured scan span. A global generation invalidating the whole column after every
small edit is a boundary, not a complete incremental solution. Keep layout's existing parent field
until a measured common structure replaces it; deleting it because the core can answer parent would
otherwise trade a direct load for a hierarchy traversal.

**Oracle:** all parent chains against preorder, deletion/reparenting and ID reuse between lease
acquisition and query. **Mutant:** omit topology generation change or keep a parent as shifted rank.
**Unused cost:** a reader-owned projection is absent in ordinary rendering; no dispatch on the base
query. Its memory, build and invalidation costs are charged to the requesting consumer.

## Settlement order

1. Capture and fix the three correctness witnesses under a small, separately reviewed card. Preserve
   pixel exactness and test semantic invalidation independently of repaint.
2. Prototype the wide sequence summary tree with existing leaves, differential edit/query gates,
   cut/join and the cost matrix; include the GPU-directory consumer cost. No coordinate, COW or motion
   rewrite bundled into its verdict. Retain the current implementation only as a benchmark oracle,
   not as a scene-selected fallback that conceals losses.
3. Coordinate growable tile/resource domains and the corresponding shader/text contracts.
4. Implement context-qualified multi-region motion and RT3 policy with exact ring-age gates.
5. Add changed-epoch publication, then decide from measured reader workloads whether COW and a
   parent projection earn their costs.

Shared-doc corrections for the coordinator: DESIGN's third spatial-level deferral is stale; the
snapshot description should distinguish changed-body copies from whole-directory scans; the mount
claim should name fragmented insertion and remapping, not ordinary contiguous initial mount. The
witnesses belong in the audit. No shared document was edited by this lane.
