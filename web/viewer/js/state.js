// Réglages et état partagé du viewer

const log = window.ViewerLog;

const VISUALIZE_BASE_EVENTS_PER_SECOND = 300;

const TILE_UNITS = 255;

const COLLECTION_PRESSURE_TIMELINE_POINTS = 180;

const CACHE_TIMELINE_SCALE_QUANTILE = 0.98;

const CACHE_TIMELINE_TAIL_FRACTION = 0.02;

const CACHE_TIMELINE_SCALE_HEADROOM = 1.08;

const LOCALITY_RED_RATE = 0.02;

const TILE_ACTIVITY_MIN_SCALE = 0.25;

const TILE_ACTIVITY_MAX_SCALE = 0.92;

const CACHE_TILE_MIN_SCALE = 0.07;

const TILE_ACTIVITY_MIN_ALPHA = 0.18;

const TILE_ACTIVITY_MAX_ALPHA = 0.82;

const TILE_ACTIVITY_GLOW_SCALE = 0.055;

const TILE_ACTIVITY_DECAY_FRACTION = 0.08;

const TILE_ACTIVITY_DECAY_MIN_EVENTS = 4000;

const VISUALIZE_MAX_BACKGROUND_NODES = 2500;

const FLASH_READ_LATENCY_MS = 1;

const FLASH_WRITE_LATENCY_MS = 3;

const HOST_REFERENCE_CPU_MHZ = 3000;

const RP2040_CPU_MHZ = 133;

const PHASE_PREFIXES = [
  ["heap_push", "Heap · push"],
  ["heap_pop", "Heap · pop"],
  ["done_get", "Done · get"],
  ["done_insert", "Done · insert"],
  ["done_remove", "Done · remove"],
  ["previous_get", "Previous · get"],
  ["previous_insert", "Previous · insert"],
  ["global_push", "Global · push"],
  ["global_pop", "Global · pop"],
  ["local_push", "Local · push"],
  ["local_pop", "Local · pop"],
  ["states_get", "State (Cost + Previous) · get"],
  ["states_insert", "State (Cost + Previous) · insert"],
  ["states_remove", "State (Cost + Previous) · remove"],
  ["neighbors_open", "Neighbors · open"],
  ["neighbors_next", "Neighbors · next"],
  ["rebuild", "Rebuild"],
];

const THEME_DEFS = [
  {
    id: "time",
    title: "Temps & modèle",
    primaryMetric: "search_ms",
    primaryLabel: "durée de recherche",
    description: "Comparaison TCEPA avec l’algorithme de référence, décomposition hôte/E/S et extrapolation RP2040.",
    primary: true,
  },
  {
    id: "result",
    title: "Résultat & configuration",
    primaryMetric: "cost",
    primaryLabel: "coût",
    description: "Résultat fonctionnel et configuration exacte de l’exécution.",
    primary: false,
  },
  {
    id: "work",
    title: "Travail algorithmique",
    primaryMetric: "algorithmic_work",
    primaryLabel: "appels instrumentés",
    description: "Volume d’appels instrumentés du graphe, de la frontière, de l’état et de l’heuristique. Ce compteur n’est pas un coût CPU pondéré.",
    primary: false,
  },
  {
    id: "memory",
    title: "Flash & cache",
    primaryMetric: "search_flash_reads",
    primaryLabel: "lectures flash",
    description: "Comportement du cache et accès flash de l’exécution instrumentée.",
    primary: false,
  },
  {
    id: "locality",
    title: "Localité réelle",
    primaryMetric: "visited_cross_tile_rate",
    primaryLabel: "visited cross",
    description: "Fréquence et amplitude des changements de tile du flux réellement exécuté.",
    primary: false,
  },
];

const THEME_SUMMARY_METRICS = {
  result: [],
  time: [],
  memory: [],
  work: [
    ["algorithmic_work", "appels instrumentés"],
    ["graph_edge_work", "travail sur le graphe"],
    ["visited_per_path_node", "visited / path node"],
    ["neighbors_skipped_ratio", "neighbors skipped"],
  ],
  locality: [
    ["visited_cross_tile_rate", "visited cross"],
    ["avg_visited_tile_jump", "avg visited jump"],
    ["neighbors_next_cross_tile_rate", "neighbors next cross"],
    ["edges_from_border_node_rate", "edges from border nodes"],
  ],
};

const state = {
  mode: "stats",
  snapshot: null,
  nodes: [],
  path: [],
  centerX: 0,
  centerY: 0,
  scale: 1,
  dragging: false,
  lastX: 0,
  lastY: 0,
  liveSettled: [],
  liveCurrent: null,
  liveRunning: false,
  benchConfig: null,
  benchSelection: {
    swap_profile: "256_256",
    scenario: "case_001",
    algo: "astar_lazy",
    baseline_algo: "astar_lazy",
    gps_layout: "base",
    graph_storage: "tile_dicho",
    neighbor_access: "tile_segments",
    exploration_policy: "strict",
    bucket_state: "coupled",
    heuristic: "standard",
  },
  benchLastResponse: null,
  benchRequestId: 0,
  visualizeRun: null,
  visualizeEvents: [],
  visualizeHasRamSnapshots: false,
  visualizeCursor: 0,
  visualizePlaying: false,
  visualizeFrame: null,
  visualizeLastFrameAt: 0,
  visualizeCarry: 0,
  visualizeSpeed: 1,
  visualizeSpeedMode: "multiplier",
  visualizeRealDurationMs: null,
  visualizeRealDurationSource: "",
  visualizeCurrentEvent: null,
  visualizeCollections: {},
  visualizeCollectionPressure: {},
  visualizeCacheTimelineMode: "misses",
  visualizeCacheTimelineLayout: "overlay",
  visualizeLeftCollapsed: false,
  visualizeRightCollapsed: false,
  visualizeCacheTimelineAvailable: false,
  visualizeCacheTimelineStride: 1,
  visualizeCacheTimelineMaxTick: 0,
  visualizeCacheTimelineCurrentTick: 0,
  visualizeCacheTimelineBuckets: new Map(),
  visualizeCacheTimelineReferenceBuckets: new Map(),
  visualizeCacheTimelineSources: [],
  visualizeCacheSourceTotals: {},
  visualizeRamState: null,
  visualizeCacheZones: new Map(),
  visualizeTotalCacheHits: 0,
  visualizeTotalCacheMisses: 0,
  visualizeMaxZoneAccesses: 0,
  visualizeZoneAccessReference: { minimum: 1, maximum: 1 },
  visualizeOpenSet: new Map(),
  visualizeOpenSetTiles: new Map(),
  visualizeOpenSetPeakReference: { minimum: 1, maximum: 1 },
  visualizeSpatialMode: "locality",
  visualizeSelectionKey: "",
  visualizeSpatialState: null,
};

export {
  CACHE_TILE_MIN_SCALE,
  CACHE_TIMELINE_SCALE_HEADROOM,
  CACHE_TIMELINE_SCALE_QUANTILE,
  CACHE_TIMELINE_TAIL_FRACTION,
  COLLECTION_PRESSURE_TIMELINE_POINTS,
  FLASH_READ_LATENCY_MS,
  FLASH_WRITE_LATENCY_MS,
  HOST_REFERENCE_CPU_MHZ,
  LOCALITY_RED_RATE,
  PHASE_PREFIXES,
  RP2040_CPU_MHZ,
  THEME_DEFS,
  THEME_SUMMARY_METRICS,
  TILE_ACTIVITY_DECAY_FRACTION,
  TILE_ACTIVITY_DECAY_MIN_EVENTS,
  TILE_ACTIVITY_GLOW_SCALE,
  TILE_ACTIVITY_MAX_ALPHA,
  TILE_ACTIVITY_MAX_SCALE,
  TILE_ACTIVITY_MIN_ALPHA,
  TILE_ACTIVITY_MIN_SCALE,
  TILE_UNITS,
  VISUALIZE_BASE_EVENTS_PER_SECOND,
  VISUALIZE_MAX_BACKGROUND_NODES,
  log,
  state,
};
