// Cache du replay : timeline, RAM et collections.

import {
  CACHE_TIMELINE_SCALE_HEADROOM,
  CACHE_TIMELINE_SCALE_QUANTILE,
  CACHE_TIMELINE_TAIL_FRACTION,
  COLLECTION_PRESSURE_TIMELINE_POINTS,
  state,
} from "../state.js";
import { benchAlgorithmDescriptor, isCountdownAlgo, usesDecoupledState } from "../controls.js";
import { cacheNodeDelta, isCacheNodeEvent } from "../map.js";
import { byId, escapeHtml, fmt, formatBytes } from "../utils.js";

const VISUALIZE_COLLECTIONS = [
  ["heap", "Heap"],
  ["done", "Done"],
  ["previous", "Previous"],
  ["global", "Global"],
  ["local", "Local"],
  ["states", "State (Cost + Previous)"],
  ["result_path", "Chemin calculé"],
  ["gps_maps", "Maps GPS"],
  ["map_stored_tiles", "Index des tuiles GPS"],
  ["map_stored_ends", "Bornes des segments GPS"],
  ["path_points", "Points du chemin GPS"],
  ["path_waypoints", "Étapes GPS"],
  ["interest_stored_tiles", "Index des intérêts GPS"],
  ["interest_stored_ends", "Bornes des intérêts GPS"],
  ["interest_data", "Données des intérêts GPS"],
  ["map_tiles", "Tuiles GPS"],
  ["map_tile_index", "Index GPS"],
  ["map_segments", "Segments GPS"],
  ["path", "Chemin GPS"],
  ["interests", "Intérêts GPS"],
  ["heights", "Altitudes GPS"],
];

const HIDDEN_VISUALIZE_COLLECTIONS = new Set([
  "gps_maps",
  "path",
  "interests",
  "heights",
]);

function isVisualizeCollectionHidden(id) {
  const indexed = state.benchSelection.graph_storage === "tile_index";
  return HIDDEN_VISUALIZE_COLLECTIONS.has(id)
    || (id === "map_tiles" && indexed)
    || (id === "map_tile_index" && !indexed);
}

const CACHE_TIMELINE_SOURCES = {
  heap: { label: "Heap", color: "#60a5fa" },
  done: { label: "Done", color: "#4ade80" },
  previous: { label: "Previous", color: "#fbbf24" },
  global: { label: "Global", color: "#60a5fa" },
  local: { label: "Local", color: "#4ade80" },
  states: { label: "State (Cost + Previous)", color: "#fb7185" },
  gps: { label: "GPS", color: "#c084fc" },
  rebuild: { label: "Reconstruction", color: "#94a3b8" },
};

const CACHE_TIMELINE_STANDARD_ORDER = ["heap", "done", "previous", "gps"];

const CACHE_TIMELINE_COUNTDOWN_ORDER = ["heap", "states", "done", "previous", "gps"];

const CACHE_TIMELINE_BUCKET_ORDER = ["heap", "global", "local", "states", "previous", "done", "gps", "rebuild"];

const COLLECTION_CACHE_SOURCES = {
  heap: "heap",
  done: "done",
  previous: "previous",
  global: "global",
  local: "local",
  states: "states",
  gps_maps: "gps",
  map_tiles: "gps",
  map_tile_index: "gps",
  map_segments: "gps",
};

function visualizeCollectionLabel(id, fallback) {
  if (id === "heap" && isCountdownAlgo(state.benchSelection?.algo) && !usesDecoupledState()) {
    return "Frontière (Heap + Cost)";
  }
  const declared = benchAlgorithmDescriptor(state.benchSelection.algo)?.replay_collections?.find((entry) => entry.id === id && (!entry.state || entry.state === state.benchSelection.bucket_state));
  if (declared) return declared.label;
  if (id === "states" && usesDecoupledState()) return "Cost";
  return fallback;
}

function cacheTimelineSourceLabel(source) {
  if (source === "states" && usesDecoupledState()) return "Cost";
  return visualizeCollectionLabel(source, CACHE_TIMELINE_SOURCES[source]?.label || source);
}

const CACHE_DELTA_KEYS = [
  "cache_hits",
  "cache_misses",
  "flash_reads",
  "flash_writes",
  "flash_allocs",
  "flash_frees",
  "flash_erases",
  "errors_io",
];

function emptyCacheDelta() {
  return Object.fromEntries(CACHE_DELTA_KEYS.map((key) => [key, 0]));
}

function copyCacheDelta(delta) {
  return Object.fromEntries(CACHE_DELTA_KEYS.map((key) => [
    key,
    Math.max(0, Number(delta?.[key] || 0)),
  ]));
}

function recordVisualizeCacheSourceDelta(source, delta) {
  const id = String(source || "");
  if (!CACHE_TIMELINE_SOURCES[id]) return;
  const current = state.visualizeCacheSourceTotals[id] || {
    delta: emptyCacheDelta(),
    events: 0,
  };
  for (const key of CACHE_DELTA_KEYS) {
    current.delta[key] += Math.max(0, Number(delta?.[key] || 0));
  }
  current.events += 1;
  state.visualizeCacheSourceTotals[id] = current;
}

function visibleVisualizeCollections() {
  const declared = benchAlgorithmDescriptor(state.benchSelection.algo)?.replay_collections;
  if (declared?.length) {
    const ids = new Set(declared.filter((entry) => !entry.state || entry.state === state.benchSelection.bucket_state).map((entry) => entry.id));
    return VISUALIZE_COLLECTIONS.filter(([id]) => !isVisualizeCollectionHidden(id) && (ids.has(id) || ["map_tiles", "map_tile_index", "map_segments"].includes(id)))
      .map(([id, label]) => [id, visualizeCollectionLabel(id, label)]);
  }
  const bucketIds = new Set(["global", "local", "states"]);
  if (usesDecoupledState()) bucketIds.add("previous");
  const standardIds = new Set(["heap", "done", "previous"]);
  if (isCountdownAlgo(state.benchSelection?.algo) && usesDecoupledState()) standardIds.add("states");
  const sharedIds = new Set(["gps_maps", "map_tiles", "map_tile_index", "map_segments", "path", "interests", "heights"]);
  const hasBucketActivity = !isCountdownAlgo(state.benchSelection?.algo) && ["global", "local", "states"].some((id) => {
    const item = state.visualizeCollections[id];
    const pressure = state.visualizeCollectionPressure[id];
    return Boolean(
      item?.cacheState
      || Number(item?.len || 0) > 0
      || item?.action !== "—"
      || Number(pressure?.snapshots || 0) > 0
    );
  });

  return VISUALIZE_COLLECTIONS
    .filter(([id]) => !isVisualizeCollectionHidden(id)
      && (sharedIds.has(id)
        || (hasBucketActivity ? bucketIds.has(id) : standardIds.has(id))))
    .map(([id, label]) => [id, visualizeCollectionLabel(id, label)]);
}

function cacheTimelineTick(event) {
  if (!isCacheNodeEvent(event) || event.tick == null) return null;
  const tick = Number(state.visualizeCacheTimelineEventAxis ? event.seq : event.tick);
  return Number.isFinite(tick) && tick >= 0 ? tick : null;
}

function emptyCacheTimelineBucket() {
  return { hits: 0, misses: 0, sources: {} };
}

function addCacheTimelineSample(buckets, event, hits, misses, stride) {
  const tick = cacheTimelineTick(event);
  const source = String(event?.source || "");
  if (tick == null || !CACHE_TIMELINE_SOURCES[source]) return;

  const bucketIndex = Math.floor(tick / Math.max(1, stride));
  const bucket = buckets.get(bucketIndex) || emptyCacheTimelineBucket();
  const sourceBucket = bucket.sources[source] || { hits: 0, misses: 0 };

  bucket.hits += hits;
  bucket.misses += misses;
  sourceBucket.hits += hits;
  sourceBucket.misses += misses;
  bucket.sources[source] = sourceBucket;
  buckets.set(bucketIndex, bucket);
}

function configureVisualizeCacheTimeline(events) {
  state.visualizeHasRamSnapshots = (events || []).some((event) => event.ram_state);
  // Les premiers replays Window enregistraient chaque événement de cache au tick 1.
  // Conserver leur chronologie d'E/S avec la séquence enregistrée, sans inventer de ticks.
  const cacheEvents = (events || []).filter(isCacheNodeEvent);
  state.visualizeCacheTimelineEventAxis = cacheEvents.length > 1
    && cacheEvents.every((event) => event.tick === cacheEvents[0].tick)
    && (events || []).filter((event) => event.type === "popped").length > 1;
  const observed = new Set();
  const referenceBuckets = new Map();
  const stride = Math.max(1, Number(state.visualizeRun?.inspect_stride || 1));
  let maximumTick = 0;
  let available = false;

  for (const event of events || []) {
    const tick = cacheTimelineTick(event);
    if (tick == null) continue;
    available = true;
    maximumTick = Math.max(maximumTick, tick);
    const source = String(event.source || "");
    if (!CACHE_TIMELINE_SOURCES[source]) continue;
    observed.add(source);
    const delta = cacheNodeDelta(event);
    addCacheTimelineSample(
      referenceBuckets,
      event,
      Math.max(0, Number(delta.cache_hits || 0)),
      Math.max(0, Number(delta.cache_misses || 0)),
      stride,
    );
  }

  const bucket = ["global", "local", "states"].some((source) => observed.has(source));
  const order = isCountdownAlgo(state.benchSelection?.algo)
    ? CACHE_TIMELINE_COUNTDOWN_ORDER
    : bucket ? CACHE_TIMELINE_BUCKET_ORDER : CACHE_TIMELINE_STANDARD_ORDER;

  state.visualizeCacheTimelineAvailable = available;
  state.visualizeCacheTimelineStride = stride;
  state.visualizeCacheTimelineMaxTick = maximumTick;
  state.visualizeCacheTimelineCurrentTick = 0;
  state.visualizeCacheTimelineBuckets = new Map();
  state.visualizeCacheTimelineReferenceBuckets = referenceBuckets;
  state.visualizeCacheTimelineSources = order.filter((source) => observed.has(source));
}

function resetVisualizeCacheTimeline() {
  state.visualizeCacheTimelineCurrentTick = 0;
  state.visualizeCacheTimelineBuckets = new Map();
}

function recordVisualizeCacheTimeline(event, hits, misses) {
  const tick = cacheTimelineTick(event);
  if (!state.visualizeCacheTimelineAvailable || tick == null) return;

  state.visualizeCacheTimelineCurrentTick = Math.max(
    state.visualizeCacheTimelineCurrentTick,
    tick,
  );
  addCacheTimelineSample(
    state.visualizeCacheTimelineBuckets,
    event,
    hits,
    misses,
    state.visualizeCacheTimelineStride,
  );
}

function cacheTimelineNiceMaximum(value, minimum = 1) {
  const safe = Math.max(minimum, Number(value || 0));
  const power = 10 ** Math.floor(Math.log10(safe));
  const normalized = safe / power;
  const steps = [1, 1.25, 1.5, 2, 2.5, 3, 4, 5, 7.5, 10];
  const step = steps.find((candidate) => normalized <= candidate) || 10;
  return Math.max(minimum, step * power);
}

function cacheTimelineQuantile(values, quantile) {
  const sorted = values
    .filter((value) => Number.isFinite(value) && value >= 0)
    .sort((left, right) => left - right);
  if (!sorted.length) return 0;
  if (sorted.length === 1) return sorted[0];

  const position = Math.max(0, Math.min(1, quantile)) * (sorted.length - 1);
  const lower = Math.floor(position);
  const upper = Math.ceil(position);
  const weight = position - lower;
  return sorted[lower] * (1 - weight) + sorted[upper] * weight;
}

function cacheTimelineRobustMaximum(values, mode) {
  const finite = values.filter((value) => Number.isFinite(value) && value >= 0);
  const minimum = mode === "rate" ? 0.25 : 1;
  if (!finite.length) return minimum;

  const tailCount = finite.length >= 8
    ? Math.max(1, Math.floor(finite.length * CACHE_TIMELINE_TAIL_FRACTION))
    : 0;
  const core = tailCount > 0 ? finite.slice(0, -tailCount) : finite;
  const quantile = cacheTimelineQuantile(
    core.length ? core : finite,
    CACHE_TIMELINE_SCALE_QUANTILE,
  );
  return cacheTimelineNiceMaximum(
    Math.max(minimum, quantile * CACHE_TIMELINE_SCALE_HEADROOM),
    minimum,
  );
}

function cacheTimelineMetric(sample, mode) {
  const hits = Math.max(0, Number(sample?.hits || 0));
  const misses = Math.max(0, Number(sample?.misses || 0));
  if (mode === "rate") {
    const accesses = hits + misses;
    return accesses > 0 ? (misses / accesses) * 100 : null;
  }
  return misses;
}

function cacheTimelinePath(values, xForIndex, yForValue) {
  const paths = [];
  let current = [];

  for (let index = 0; index < values.length; index += 1) {
    const value = values[index];
    if (value == null || !Number.isFinite(value)) {
      if (current.length) paths.push(current);
      current = [];
      continue;
    }
    current.push({ x: xForIndex(index), y: yForValue(value) });
  }
  if (current.length) paths.push(current);

  return paths.map((points) => points
    .map((point, index) => `${index ? "L" : "M"}${point.x.toFixed(2)},${point.y.toFixed(2)}`)
    .join(" "));
}

function cacheTimelineSeriesFromBuckets(buckets, bucketCount, mode) {
  const samples = Array.from({ length: Math.max(1, bucketCount) }, (_, index) => (
    buckets.get(index) || emptyCacheTimelineBucket()
  ));
  const aggregate = {
    id: "all",
    label: "Global",
    color: "#f8fafc",
    values: samples.map((bucket) => cacheTimelineMetric(bucket, mode)),
  };
  const sources = state.visualizeCacheTimelineSources.map((source) => ({
    id: source,
    label: cacheTimelineSourceLabel(source),
    color: CACHE_TIMELINE_SOURCES[source].color,
    values: samples.map((bucket) => cacheTimelineMetric(bucket.sources[source], mode)),
  }));
  return { aggregate, sources };
}

function cacheTimelineSeries() {
  const stride = Math.max(1, state.visualizeCacheTimelineStride);
  const maximumBucket = Math.floor(state.visualizeCacheTimelineMaxTick / stride);
  const currentBucket = Math.min(
    maximumBucket,
    Math.floor(state.visualizeCacheTimelineCurrentTick / stride),
  );
  return {
    maximumBucket,
    currentBucket,
    ...cacheTimelineSeriesFromBuckets(
      state.visualizeCacheTimelineBuckets,
      currentBucket + 1,
      state.visualizeCacheTimelineMode,
    ),
  };
}

function cacheTimelineReferenceSeries(mode) {
  const stride = Math.max(1, state.visualizeCacheTimelineStride);
  const maximumBucket = Math.floor(state.visualizeCacheTimelineMaxTick / stride);
  return cacheTimelineSeriesFromBuckets(
    state.visualizeCacheTimelineReferenceBuckets,
    maximumBucket + 1,
    mode,
  );
}

function renderCacheTimelineGlobalSparkline(aggregate, referenceAggregate, maximumBucket, cursorProgress) {
  const width = 720;
  const height = 54;
  const geometry = { left: 56, right: 700, top: 6, bottom: 44 };
  const scaleMaximum = cacheTimelineRobustMaximum(
    referenceAggregate.values,
    state.visualizeCacheTimelineMode,
  );
  const xForIndex = (index) => geometry.left
    + (maximumBucket > 0 ? index / maximumBucket : 0) * (geometry.right - geometry.left);
  const yForValue = (value) => geometry.bottom
    - Math.min(1, Math.max(0, value / scaleMaximum)) * (geometry.bottom - geometry.top);
  const paths = cacheTimelinePath(aggregate.values, xForIndex, yForValue);
  const cursorX = geometry.left + cursorProgress * (geometry.right - geometry.left);

  return `
      <div class="cache-timeline-global">
        <span class="cache-timeline-global-label"><i></i>Global</span>
        <svg class="cache-timeline-global-chart" viewBox="0 0 ${width} ${height}" aria-label="Forme globale des accès cache">
          <line class="cache-timeline-global-baseline" x1="${geometry.left}" y1="${geometry.bottom}" x2="${geometry.right}" y2="${geometry.bottom}"></line>
          ${paths.map((path) => `<path class="cache-timeline-global-line" d="${path}"></path>`).join("")}
          <line class="cache-timeline-cursor" x1="${cursorX}" y1="${geometry.top}" x2="${cursorX}" y2="${geometry.bottom}"></line>
        </svg>
      </div>
    `;
}

function renderCacheTimelineOverlay(details, referenceDetails, maximumBucket, cursorProgress, mode) {
  const width = 720;
  const height = 272;
  const geometry = { left: 58, right: 700, top: 18, bottom: 225 };
  const referenceValues = referenceDetails.flatMap((entry) => entry.values);
  const scaleMaximum = cacheTimelineRobustMaximum(referenceValues, mode);
  const xForIndex = (index) => geometry.left
    + (maximumBucket > 0 ? index / maximumBucket : 0) * (geometry.right - geometry.left);
  const yForValue = (value) => geometry.bottom
    - Math.min(1, Math.max(0, value / scaleMaximum)) * (geometry.bottom - geometry.top);
  const gridRates = [0, 0.25, 0.5, 0.75, 1];
  const unit = mode === "rate" ? "%" : "misses";
  const cursorX = geometry.left + cursorProgress * (geometry.right - geometry.left);

  return `
      <svg class="cache-timeline-chart" viewBox="0 0 ${width} ${height}" aria-label="${mode === "rate" ? "Taux de miss" : "Misses"} au cours du replay">
        ${gridRates.map((rate) => {
    const y = geometry.bottom - rate * (geometry.bottom - geometry.top);
    const value = scaleMaximum * rate;
    return `
            <line class="cache-timeline-grid" x1="${geometry.left}" y1="${y}" x2="${geometry.right}" y2="${y}"></line>
            <text class="cache-timeline-axis" x="${geometry.left - 9}" y="${y + 4}" text-anchor="end">${fmt(value, mode === "rate" ? 2 : 0)}</text>
          `;
  }).join("")}
        ${details.map((entry) => cacheTimelinePath(entry.values, xForIndex, yForValue).map((path) => `
          <path class="cache-timeline-line" style="--series-color:${entry.color}" d="${path}"></path>
        `).join("")).join("")}
        <line class="cache-timeline-cursor" x1="${cursorX}" y1="${geometry.top}" x2="${cursorX}" y2="${geometry.bottom}"></line>
        <text class="cache-timeline-axis x" x="${geometry.left}" y="${geometry.bottom + 25}">début</text>
        <text class="cache-timeline-axis x" x="${geometry.right}" y="${geometry.bottom + 25}" text-anchor="end">fin</text>
        <text class="cache-timeline-unit" x="${geometry.left}" y="${geometry.top - 8}">${unit}</text>
      </svg>
      <div class="cache-timeline-fixed-scale">Échelle fixe : 0–${fmt(scaleMaximum, mode === "rate" ? 2 : 0)} ${unit}</div>
    `;
}

function renderCacheTimelineSplit(details, referenceDetails, maximumBucket, cursorProgress, mode) {
  const width = 720;
  const height = 300;
  const geometry = { left: 76, right: 700, top: 10, bottom: 266 };
  const gap = 6;
  const count = Math.max(1, details.length);
  const bandHeight = (geometry.bottom - geometry.top - gap * (count - 1)) / count;
  const xForIndex = (index) => geometry.left
    + (maximumBucket > 0 ? index / maximumBucket : 0) * (geometry.right - geometry.left);
  const referenceById = new Map(referenceDetails.map((entry) => [entry.id, entry]));
  const unit = mode === "rate" ? "%" : "misses";
  const cursorX = geometry.left + cursorProgress * (geometry.right - geometry.left);

  const bands = details.map((entry, index) => {
    const bandTop = geometry.top + index * (bandHeight + gap);
    const bandBottom = bandTop + bandHeight;
    const reference = referenceById.get(entry.id) || entry;
    const maximum = cacheTimelineRobustMaximum(reference.values, mode);
    const yForValue = (value) => bandBottom
      - Math.min(1.12, Math.max(0, value / maximum)) * bandHeight;
    return {
      ...entry,
      bandTop,
      bandBottom,
      maximum,
      paths: cacheTimelinePath(entry.values, xForIndex, yForValue),
    };
  });

  return `
      <svg class="cache-timeline-chart split" viewBox="0 0 ${width} ${height}" aria-label="Courbes cache séparées par collection">
        ${bands.map((entry) => `
          <line class="cache-timeline-band" x1="${geometry.left}" y1="${entry.bandBottom}" x2="${geometry.right}" y2="${entry.bandBottom}"></line>
          <text class="cache-timeline-band-label" x="${geometry.left - 8}" y="${entry.bandTop + 13}" text-anchor="end" style="--series-color:${entry.color}">${escapeHtml(entry.label)}</text>
          <text class="cache-timeline-band-scale" x="${geometry.right - 2}" y="${entry.bandTop + 12}" text-anchor="end">0–${fmt(entry.maximum, mode === "rate" ? 2 : 0)} ${unit}</text>
          ${entry.paths.map((path) => `<path class="cache-timeline-line" style="--series-color:${entry.color}" d="${path}"></path>`).join("")}
        `).join("")}
        <line class="cache-timeline-cursor" x1="${cursorX}" y1="${geometry.top}" x2="${cursorX}" y2="${geometry.bottom}"></line>
        <text class="cache-timeline-axis x" x="${geometry.left}" y="${geometry.bottom + 25}">début</text>
        <text class="cache-timeline-axis x" x="${geometry.right}" y="${geometry.bottom + 25}" text-anchor="end">fin</text>
      </svg>
      <div class="cache-timeline-fixed-scale">Échelles fixes propres à chaque collection</div>
    `;
}

function renderVisualizeCacheTimeline() {
  const root = byId("visualize-cache-timeline");
  if (!root) return;
  root.hidden = state.mode !== "visualize";

  const mode = state.visualizeCacheTimelineMode;
  const layout = state.visualizeCacheTimelineLayout;
  const buttons = `
      <div class="cache-timeline-switches">
        <div class="cache-timeline-mode" role="group" aria-label="Métrique temporelle du cache">
          <button type="button" data-cache-timeline-mode="misses" class="${mode === "misses" ? "active" : ""}">Misses</button>
          <button type="button" data-cache-timeline-mode="rate" class="${mode === "rate" ? "active" : ""}">Taux de miss</button>
        </div>
        <div class="cache-timeline-mode" role="group" aria-label="Disposition des courbes cache">
          <button type="button" data-cache-timeline-layout="overlay" class="${layout === "overlay" ? "active" : ""}">Superposé</button>
          <button type="button" data-cache-timeline-layout="split" class="${layout === "split" ? "active" : ""}">Split</button>
        </div>
      </div>
    `;

  if (!state.visualizeCacheTimelineAvailable) {
    root.innerHTML = `
        <div class="cache-timeline-head">
          <div><h2>Cache dans le temps</h2><span>fenêtres de ${fmt(state.visualizeCacheTimelineStride)} ${state.visualizeCacheTimelineEventAxis ? "événements · ticks absents de cet ancien replay" : "ticks"}</span></div>
          ${buttons}
        </div>
        <div class="cache-timeline-unavailable">Version sans timeline cache.</div>
      `;
    return;
  }

  const { maximumBucket, aggregate, sources } = cacheTimelineSeries();
  const reference = cacheTimelineReferenceSeries(mode);
  const cursorProgress = state.visualizeCacheTimelineMaxTick > 0
    ? Math.max(0, Math.min(1, state.visualizeCacheTimelineCurrentTick / state.visualizeCacheTimelineMaxTick))
    : 0;
  const currentBucket = state.visualizeCacheTimelineBuckets.get(
    Math.floor(state.visualizeCacheTimelineCurrentTick / Math.max(1, state.visualizeCacheTimelineStride)),
  ) || emptyCacheTimelineBucket();
  const currentValue = cacheTimelineMetric(currentBucket, mode);
  const currentLabel = mode === "rate"
    ? (currentValue == null ? "—" : `${fmt(currentValue, 2)} %`)
    : fmt(currentValue || 0);

  root.innerHTML = `
      <div class="cache-timeline-head">
        <div>
          <h2>Cache dans le temps</h2>
          <span>fenêtres de ${fmt(state.visualizeCacheTimelineStride)} ${state.visualizeCacheTimelineEventAxis ? "événements · axe séquence (ancien replay)" : "ticks"} · actuel ${currentLabel}</span>
        </div>
        ${buttons}
      </div>
      ${renderCacheTimelineGlobalSparkline(
    aggregate,
    reference.aggregate,
    maximumBucket,
    cursorProgress,
  )}
      <div class="cache-timeline-legend">
        ${sources.map((entry) => `
          <span><i style="--series-color:${entry.color}"></i>${escapeHtml(entry.label)}</span>
        `).join("")}
      </div>
      ${layout === "split"
      ? renderCacheTimelineSplit(sources, reference.sources, maximumBucket, cursorProgress, mode)
      : renderCacheTimelineOverlay(sources, reference.sources, maximumBucket, cursorProgress, mode)}
    `;
}

function emptyCollectionPressureState() {
  const pressure = {};
  for (const [id] of VISUALIZE_COLLECTIONS) {
    pressure[id] = {
      snapshots: 0,
      sumCachedBytes: 0,
      peakCachedBytes: 0,
      lastCachedBytes: 0,
      validHotSnapshots: 0,
      sumHotShare: 0,
      lastHotShare: null,
      capacityBytes: 0,
      samples: [],
      lastSampleBucket: -1,
    };
  }
  return pressure;
}

function recordCollectionPressure(collection, cacheState, ramState, eventSeq) {
  const current = state.visualizeCollectionPressure[collection];
  if (!current || !cacheState) return;

  const cachedBytes = collectionResidentBytes(cacheState);
  // swap-cache expose le rang dans le cache d'un bloc résident, pas une identité stable du bloc.
  // Les octets résidents sont réels pour l'instantané courant, tandis que la chaleur
  // et la stabilité restent calculées par entrée entre les instantanés.
  const cachedEntries = Math.max(0, Number(cacheState.entries_cached || 0));
  const keptEntries = Math.max(0, Number(cacheState.kept_entries || 0));
  const capacityBytes = ramCapacityBytes(ramState) || current.capacityBytes || 0;
  const hasHeatData = cacheState.entries_cached != null && cacheState.kept_entries != null;
  const hasPreviousSnapshot = current.snapshots > 0;
  const hotShare = hasPreviousSnapshot && hasHeatData && cachedEntries > 0
    ? Math.max(0, Math.min(1, keptEntries / cachedEntries))
    : null;
  const hotBytes = hotShare == null ? null : cachedBytes * hotShare;

  current.snapshots += 1;
  current.sumCachedBytes += cachedBytes;
  current.peakCachedBytes = Math.max(current.peakCachedBytes, cachedBytes);
  current.lastCachedBytes = cachedBytes;
  current.lastHotShare = hotShare;
  if (hotShare != null) {
    current.validHotSnapshots += 1;
    current.sumHotShare += hotShare;
  }
  if (capacityBytes > 0) current.capacityBytes = capacityBytes;

  const totalEvents = Math.max(1, state.visualizeEvents.length - 1);
  const seq = Math.max(0, Number(eventSeq ?? state.visualizeCursor ?? 0));
  const progress = Math.max(0, Math.min(1, seq / totalEvents));
  const bucket = Math.min(
    COLLECTION_PRESSURE_TIMELINE_POINTS - 1,
    Math.floor(progress * COLLECTION_PRESSURE_TIMELINE_POINTS),
  );
  const sample = {
    progress,
    cachedBytes,
    hotBytes,
    hotShare,
    capacityBytes: current.capacityBytes,
  };

  if (bucket === current.lastSampleBucket && current.samples.length > 0) {
    current.samples[current.samples.length - 1] = sample;
  } else {
    current.samples.push(sample);
    current.lastSampleBucket = bucket;
  }
}

function collectionResidentBytes(cacheState) {
  if (!cacheState) return 0;
  return Math.max(0, Number(
    cacheState.resident_bytes
    ?? cacheState.estimated_cached_bytes
    ?? cacheState.cached_bytes
    ?? 0,
  ));
}

function collectionHasMeasuredResidency(cacheState) {
  return cacheState?.resident_blocks != null && cacheState?.resident_bytes != null;
}

function ramCapacityBytes(ramState) {
  return Math.max(0, Number(ramState?.cache_capacity_bytes ?? ramState?.capacity ?? 0));
}

function ramResidentBytes(ramState) {
  return Math.max(0, Number(
    ramState?.observed_resident_bytes
    ?? ramState?.resident_bytes
    ?? ramState?.resident_size
    ?? 0,
  ));
}

function ramResidentBlocks(ramState) {
  return Math.max(0, Number(
    ramState?.observed_resident_blocks
    ?? ramState?.resident_blocks
    ?? 0,
  ));
}

function emptyCollectionReplayState() {
  const collections = {};
  for (const [id] of VISUALIZE_COLLECTIONS) {
    collections[id] = {
      len: 0,
      action: "—",
      changed: false,
      delta: emptyCacheDelta(),
      deltaAvailable: false,
      cacheState: null,
    };
  }
  return collections;
}

function collectionPressureRows(capacityBytes) {
  return visibleVisualizeCollections().map(([id, label]) => {
    const pressure = state.visualizeCollectionPressure[id];
    if (!pressure || pressure.snapshots === 0) return null;

    const capacity = Math.max(1, Number(pressure.capacityBytes || capacityBytes || 0));
    const currentBytes = Math.max(0, Number(pressure.lastCachedBytes || 0));
    const averageBytes = pressure.sumCachedBytes / pressure.snapshots;
    const peakBytes = pressure.peakCachedBytes;
    const currentHotShare = Number.isFinite(pressure.lastHotShare)
      ? Math.max(0, Math.min(1, pressure.lastHotShare))
      : null;
    const averageHotShare = pressure.validHotSnapshots > 0
      ? Math.max(0, Math.min(1, pressure.sumHotShare / pressure.validHotSnapshots))
      : null;

    return {
      id,
      label,
      pressure,
      capacity,
      currentBytes,
      averageBytes,
      peakBytes,
      currentHotShare,
      averageHotShare,
      currentRate: Math.min(100, (currentBytes / capacity) * 100),
      averageRate: Math.min(100, (averageBytes / capacity) * 100),
      peakRate: Math.min(100, (peakBytes / capacity) * 100),
    };
  })
    .filter(Boolean)
    .sort((left, right) => right.peakRate - left.peakRate);
}

function renderCollectionPressureMiniChart(row, yMax, showXAxis) {
  const width = 420;
  const height = 74;
  const geometry = { left: 4, right: width - 4, top: 5, bottom: 57 };
  const samples = row.pressure.samples;
  if (!samples.length) return "";

  const point = (sample, bytes) => {
    const capacity = Math.max(1, Number(sample.capacityBytes || row.capacity));
    const rate = Math.min(yMax, (Math.max(0, Number(bytes || 0)) / capacity) * 100);
    return {
      x: geometry.left + Number(sample.progress || 0) * (geometry.right - geometry.left),
      y: geometry.bottom - (rate / yMax) * (geometry.bottom - geometry.top),
    };
  };
  const totalPoints = samples.map((sample) => point(sample, sample.cachedBytes));
  const totalPath = totalPoints
    .map((value, index) => `${index ? "L" : "M"}${value.x.toFixed(2)},${value.y.toFixed(2)}`)
    .join(" ");
  const firstTotal = totalPoints[0];
  const lastTotal = totalPoints[totalPoints.length - 1];
  const totalArea = `M${firstTotal.x.toFixed(2)},${geometry.bottom} ${totalPoints
    .map((value) => `L${value.x.toFixed(2)},${value.y.toFixed(2)}`)
    .join(" ")} L${lastTotal.x.toFixed(2)},${geometry.bottom} Z`;

  const heatSegments = [];
  let currentSegment = [];
  for (const sample of samples) {
    if (sample.hotBytes == null) {
      if (currentSegment.length > 0) heatSegments.push(currentSegment);
      currentSegment = [];
      continue;
    }
    currentSegment.push({
      hot: point(sample, sample.hotBytes),
      total: point(sample, sample.cachedBytes),
    });
  }
  if (currentSegment.length > 0) heatSegments.push(currentSegment);

  const hotPaths = heatSegments.map((segment) => segment
    .map(({ hot }, index) => `${index ? "L" : "M"}${hot.x.toFixed(2)},${hot.y.toFixed(2)}`)
    .join(" "));
  const renewedAreas = heatSegments.map((segment) => {
    const totalOutline = segment
      .map(({ total }, index) => `${index ? "L" : "M"}${total.x.toFixed(2)},${total.y.toFixed(2)}`)
      .join(" ");
    const hotReturn = [...segment]
      .reverse()
      .map(({ hot }) => `L${hot.x.toFixed(2)},${hot.y.toFixed(2)}`)
      .join(" ");
    return `${totalOutline} ${hotReturn} Z`;
  });
  const lastHeatSegment = heatSegments[heatSegments.length - 1];
  const lastHot = row.currentHotShare != null && lastHeatSegment?.length
    ? lastHeatSegment[lastHeatSegment.length - 1].hot
    : null;
  const averageY = geometry.bottom
    - (Math.min(yMax, row.averageRate) / yMax) * (geometry.bottom - geometry.top);
  const cursorProgress = state.visualizeEvents.length > 0
    ? Math.max(0, Math.min(1, state.visualizeCursor / state.visualizeEvents.length))
    : 0;
  const cursorX = geometry.left + cursorProgress * (geometry.right - geometry.left);

  return `
      <svg class="collection-pressure-mini-chart" viewBox="0 0 ${width} ${height}" aria-label="Evolution de la mémoire résidente, conservée et renouvelée pour ${escapeHtml(row.label)}">
        <line class="collection-pressure-mini-grid" x1="${geometry.left}" y1="${geometry.bottom}" x2="${geometry.right}" y2="${geometry.bottom}"></line>
        <path class="collection-pressure-total-area" d="${totalArea}"></path>
        ${renewedAreas.map((area) => `<path class="collection-pressure-renewed-area" d="${area}"></path>`).join("")}
        <line class="collection-pressure-average" x1="${geometry.left}" y1="${averageY}" x2="${geometry.right}" y2="${averageY}"></line>
        <path class="collection-pressure-outline total" d="${totalPath}"></path>
        ${hotPaths.map((hotPath) => `<path class="collection-pressure-outline hot" d="${hotPath}"></path>`).join("")}
        <circle class="collection-pressure-last total" cx="${lastTotal.x}" cy="${lastTotal.y}" r="2.7"></circle>
        ${lastHot ? `<circle class="collection-pressure-last hot" cx="${lastHot.x}" cy="${lastHot.y}" r="2.4"></circle>` : ""}
        <line class="collection-pressure-cursor" x1="${cursorX}" y1="${geometry.top}" x2="${cursorX}" y2="${geometry.bottom}"></line>
        ${showXAxis ? `
          <text class="collection-pressure-axis-label" x="${geometry.left}" y="69">début</text>
          <text class="collection-pressure-axis-label" x="${geometry.right}" y="69" text-anchor="end">fin</text>
        ` : ""}
      </svg>
    `;
}

function renderCollectionPressure(capacityBytes) {
  const rows = collectionPressureRows(capacityBytes);
  if (!rows.length) return "";

  const maxRate = Math.max(
    0,
    ...rows.flatMap((row) => row.pressure.samples.map((sample) => {
      const capacity = Math.max(1, Number(sample.capacityBytes || row.capacity));
      return (Number(sample.cachedBytes || 0) / capacity) * 100;
    })),
  );
  const yMax = Math.min(100, Math.max(20, Math.ceil(maxRate / 7) * 5));

  return `
      <section class="collection-pressure">
        <div class="collection-pressure-head">
          <strong>Evolution du cache dans le temps</strong>
          <span>Echelle pour ce run : 0–${fmt(yMax)} %</span>
        </div>
        <div class="collection-pressure-key">
          <span><i class="total"></i>données résidentes</span>
          <span><i class="hot"></i>conservées</span>
          <span><i class="renewed"></i>nouvelles</span>
          <span><i class="average"></i>moyenne</span>
        </div>
        <div class="collection-pressure-cards">
          ${rows.map((row, index) => `
            <article class="collection-pressure-card collection-${row.id}">
              <div class="collection-pressure-card-head">
                <strong>${escapeHtml(row.label)}</strong>
                <span>${formatBytes(row.currentBytes)} · ${fmt(row.currentRate, 1)} % du cache</span>
              </div>
              <div class="collection-pressure-card-body">
                <div class="collection-pressure-plot">
                  ${renderCollectionPressureMiniChart(row, yMax, index === rows.length - 1)}
                </div>
                <div class="collection-pressure-side">
                  <div
                    class="collection-heat-ring${row.currentHotShare == null ? " unavailable" : ""}"
                    style="--hot-share:${row.currentHotShare == null ? "0%" : `${(row.currentHotShare * 100).toFixed(2)}%`}"
                    title="Part des entrées actuellement résidentes qui étaient déjà présentes au snapshot précédent"
                  >
                    <strong>${row.currentHotShare == null ? "—" : `${fmt(row.currentHotShare * 100, 0)} %`}</strong>
                    <span>chauds</span>
                  </div>
                  <dl class="collection-pressure-kpis">
                    <div><dt>actuel</dt><dd>${fmt(row.currentRate, 1)} %</dd></div>
                    <div><dt>moyenne</dt><dd>${fmt(row.averageRate, 1)} %</dd></div>
                    <div><dt>pic</dt><dd>${fmt(row.peakRate, 1)} %</dd></div>
                    <div><dt>stabilité</dt><dd>${row.averageHotShare == null ? "—" : `${fmt(row.averageHotShare * 100, 1)} %`}</dd></div>
                  </dl>
                </div>
              </div>
            </article>
          `).join("")}
        </div>
      </section>
    `;
}

function renderVisualizeRam() {
  const root = byId("visualize-ram");
  if (!root) return;

  const ram = state.visualizeRamState;
  const capacity = ramCapacityBytes(ram);
  if (!ram || !capacity) {
    root.innerHTML = `
        <div class="ram-replay-head">
          <strong>Résidence swap-cache observée</strong>
          <span>${state.visualizeRun && !state.visualizeHasRamSnapshots ? "Snapshots absents de ce replay : relancer inspect pour mesurer la résidence RAM" : "snapshot non disponible"}</span>
        </div>
        <div class="ram-stacked-bar"><span class="ram-segment ram-free" style="width:100%"></span></div>
      `;
    return;
  }

  const resident = Math.min(capacity, ramResidentBytes(ram));
  const residentBlocks = ramResidentBlocks(ram);
  const hasResidentBlocks = ram.observed_resident_blocks != null || ram.resident_blocks != null;
  const capacityBlocks = Math.max(0, Number(ram.cache_capacity_blocks || 0));
  const unobserved = Math.max(0, capacity - resident);
  const observedRate = capacity > 0 ? (resident / capacity) * 100 : 0;
  const blockInfo = !hasResidentBlocks ? "Nombre de blocs non enregistré dans ce replay"
    : capacityBlocks > 0
      ? `${fmt(residentBlocks)} / ${fmt(capacityBlocks)} blocs observés résidents`
      : `${fmt(residentBlocks)} blocs observés résidents`;

  const collectionLabels = new Map(VISUALIZE_COLLECTIONS);
  const collectionClass = (id) => collectionLabels.has(id) || ["other", "free"].includes(id) ? id : "other";
  const hasAtomicBreakdown = Array.isArray(ram.resident_collections);
  const measuredCollections = (hasAtomicBreakdown
    ? ram.resident_collections.map((entry) => {
      const id = String(entry.collection || "other");
      const label = collectionLabels.get(id) || id;
      return [
        id,
        visualizeCollectionLabel(id, label),
        Math.max(0, Number(entry.resident_bytes || 0)),
        entry.resident_blocks != null ? Math.max(0, Number(entry.resident_blocks)) : null,
      ];
    })
    : visibleVisualizeCollections().map(([id, label]) => [
      id,
      visualizeCollectionLabel(id, label),
      collectionHasMeasuredResidency(state.visualizeCollections[id]?.cacheState)
        ? collectionResidentBytes(state.visualizeCollections[id]?.cacheState)
        : 0,
      collectionHasMeasuredResidency(state.visualizeCollections[id]?.cacheState)
        ? Number(state.visualizeCollections[id].cacheState.resident_blocks)
        : null,
    ]))
    .filter(([, , bytes]) => bytes > 0);
  const measuredTotal = measuredCollections.reduce((sum, [, , bytes]) => sum + bytes, 0);
  // La v2.3 fournit une décomposition atomique et dédupliquée dans ram_state.
  // Les anciens fichiers de replay utilisent les instantanés par collection, qui peuvent
  // brièvement se mélanger pendant la lecture d'un groupe d'instantanés.
  const breakdownAvailable = hasAtomicBreakdown
    ? measuredTotal === resident
    : measuredTotal <= resident;
  const displayedCollections = measuredCollections.filter(([id]) => !isVisualizeCollectionHidden(id));
  const displayedTotal = displayedCollections.reduce((sum, [, , bytes]) => sum + bytes, 0);
  const attributedObserved = breakdownAvailable ? Math.max(0, resident - displayedTotal) : resident;
  const displayedBlocks = displayedCollections.reduce((sum, [, , , blocks]) => sum + Number(blocks || 0), 0);
  const observedSegments = breakdownAvailable
    ? [
      ...displayedCollections,
      ...(attributedObserved > 0 ? [["other", "Observé non détaillé", attributedObserved, hasResidentBlocks ? Math.max(0, residentBlocks - displayedBlocks) : null]] : []),
    ]
    : [["other", "Observé par PATH", resident, hasResidentBlocks ? residentBlocks : null]];
  const segments = [
    ...observedSegments,
    ["free", "Non observé / non attribué", unobserved],
  ];

  root.innerHTML = `
      <div class="ram-replay-head">
        <strong>Résidence swap-cache observée</strong>
        <span>${formatBytes(resident)} / ${formatBytes(capacity)} (${fmt(observedRate, 1)} %)</span>
      </div>
      <div class="ram-replay-sub">${escapeHtml(blockInfo)} · les blocs internes non exposés par inspect_cache restent non attribués</div>
      <div class="ram-stacked-bar">
        ${segments.map(([id, , bytes]) => `<span class="ram-segment ram-${escapeHtml(collectionClass(id))}" style="width:${capacity ? (bytes / capacity) * 100 : 0}%"></span>`).join("")}
      </div>
      <div class="ram-legend">
        ${segments.map(([id, label, bytes, blocks]) => `<span class="ram-legend-item"><i class="ram-${escapeHtml(collectionClass(id))}"></i><span>${escapeHtml(label)}</span><strong>${blocks != null ? `${fmt(blocks)} blocs · ` : ""}${formatBytes(bytes)}</strong></span>`).join("")}
      </div>
      ${renderCollectionPressure(capacity)}
    `;
}

function renderVisualizeCollections() {
  const root = byId("visualize-collections");
  if (!root) return;

  root.innerHTML = visibleVisualizeCollections().map(([id, label]) => {
    const item = state.visualizeCollections[id] || emptyCollectionReplayState()[id];
    const cache = item.cacheState;
    const total = Number(cache?.entries_total || 0);
    const cached = Number(cache?.entries_cached || 0);
    const rate = total > 0 ? Math.max(0, Math.min(100, (cached / total) * 100)) : 0;
    const displayedTotal = cache ? total : Number(item.len || 0);
    const residentLabel = cache
      ? `${fmt(cached)} / ${fmt(total)} entrées dans des blocs résidents · ${fmt(rate, 1)} %`
      : "snapshot cache non disponible";
    const measured = collectionHasMeasuredResidency(cache);
    const bytesLabel = cache
      ? measured
        ? `${fmt(cache.resident_blocks || 0)} blocs · ${formatBytes(cache.resident_bytes || 0)}`
        : `estimé ${formatBytes(cache.estimated_cached_bytes ?? cache.cached_bytes ?? 0)}`
      : "—";
    const movementLabel = cache
      ? `entrées ↗️${fmt(cache.entered_entries || 0)} · ➡️${fmt(cache.kept_entries || 0)} · ↘️${fmt(cache.exited_entries || 0)}`
      : "";
    const source = COLLECTION_CACHE_SOURCES[id];
    const sourceTotals = source ? state.visualizeCacheSourceTotals[source] : null;
    const hasCumulativeDelta = Number(sourceTotals?.events || 0) > 0;
    const displayedDelta = hasCumulativeDelta ? sourceTotals.delta : item.delta;
    const reads = Number(displayedDelta?.flash_reads || 0);
    const writes = Number(displayedDelta?.flash_writes || 0);
    const ioLabel = hasCumulativeDelta
      ? `Total · Flash R: ${fmt(reads)} · FLASH W: ${fmt(writes)}`
      : item.deltaAvailable
        ? `Instantané · Flash R: ${fmt(reads)} · FLASH W: ${fmt(writes)}`
        : "Total indisponible";

    return `
        <article class="collection-replay-card collection-${id}">
          <div class="collection-replay-head">
            <strong>${escapeHtml(label)}</strong>
            <span>${fmt(displayedTotal)} entrées</span>
          </div>
          <div class="collection-replay-cache">
            <span>${escapeHtml(residentLabel)}</span>
            <span>${escapeHtml(bytesLabel)}</span>
          </div>
          <div class="collection-replay-io">
            <span class="collection-replay-movement">
              <strong>${escapeHtml(movementLabel)}</strong>
            </span>
            <span>${escapeHtml(ioLabel)}</span>
          </div>
          <div class="collection-residency-bar"><span style="width:${rate}%"></span></div>
        </article>
      `;
  }).join("");
}

export {
  CACHE_DELTA_KEYS,
  COLLECTION_CACHE_SOURCES,
  configureVisualizeCacheTimeline,
  copyCacheDelta,
  emptyCacheDelta,
  emptyCollectionPressureState,
  emptyCollectionReplayState,
  recordCollectionPressure,
  recordVisualizeCacheSourceDelta,
  recordVisualizeCacheTimeline,
  renderVisualizeCacheTimeline,
  renderVisualizeCollections,
  renderVisualizeRam,
  resetVisualizeCacheTimeline,
};
