// Chargement, événements et lecture du replay.

import { byId, escapeHtml, fetchJson, fmt, numberField } from "../utils.js";
import { VISUALIZE_BASE_EVENTS_PER_SECOND, state } from "../state.js";
import {
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
} from "./cache.js";
import {
  addOpenSetEntry,
  cacheNodeDelta,
  draw,
  isCacheNodeEvent,
  removeOpenSetEntry,
  resetLive,
  spatialReplayReferences,
  tileCoordinates,
  tileKey,
  updateCacheActivity,
} from "../map.js";
import { renderVisualizeLayoutControls, toggleVisualizeSide } from "../controls.js";

function formatReplayDuration(milliseconds) {
  const duration = Math.max(0, Number(milliseconds || 0));
  if (duration < 1000) return `${fmt(duration, duration < 100 ? 1 : 0)} ms`;
  return `${fmt(duration / 1000, duration < 10000 ? 2 : 1)} s`;
}

function updateVisualizeSpeedControl(autoSelectReal = false) {
  const select = byId("visualize-speed");
  if (!select) return;

  const realOption = select.querySelector('option[value="real"]');
  if (!realOption) return;

  const duration = state.visualizeRealDurationMs;
  const available = Number.isFinite(duration) && duration > 0;
  realOption.disabled = !available;
  realOption.textContent = available
    ? `Réelle · ${formatReplayDuration(duration)}`
    : "Réelle · durée perf indisponible";

  if (available && autoSelectReal) {
    select.value = "real";
    state.visualizeSpeedMode = "real";
    state.visualizeCarry = 0;
    state.visualizeLastFrameAt = 0;
    return;
  }

  if (!available) {
    select.value = "1";
    state.visualizeSpeedMode = "multiplier";
    state.visualizeSpeed = 1;
    state.visualizeCarry = 0;
    state.visualizeLastFrameAt = 0;
  }
}

function setVisualizeRealDuration(response) {
  const metrics = response?.selected?.metrics;
  const duration = numberField(metrics, ["search_ms", "searchMs"]);

  state.visualizeRealDurationMs = duration != null && duration > 0 ? duration : null;
  state.visualizeRealDurationSource = state.visualizeRealDurationMs != null
    ? String(response?.selected?.dataset_label || "temps perf")
    : "";

  updateVisualizeSpeedControl(true);
  renderVisualizeRunMeta();
  renderVisualizeCacheTimeline();
}

function fetchVisualizeRealDuration(params, selectionKey) {
  state.visualizeRealDurationMs = null;
  state.visualizeRealDurationSource = "";
  updateVisualizeSpeedControl();

  fetchJson("/api/bench/result?" + params.toString())
    .then((response) => {
      if (state.visualizeSelectionKey !== selectionKey) return;
      setVisualizeRealDuration(response);
    })
    .catch(() => {
      if (state.visualizeSelectionKey !== selectionKey) return;
      setVisualizeRealDuration(null);
    });
}

function visualizeEventsPerSecond() {
  if (state.visualizeSpeedMode === "real") {
    const duration = Number(state.visualizeRealDurationMs || 0);
    if (duration > 0 && state.visualizeEvents.length > 0) {
      return state.visualizeEvents.length / (duration / 1000);
    }
  }

  return VISUALIZE_BASE_EVENTS_PER_SECOND * state.visualizeSpeed;
}

function visualizeSpeedLabel() {
  if (state.visualizeSpeedMode === "real" && state.visualizeRealDurationMs != null) {
    return `vitesse réelle · ${formatReplayDuration(state.visualizeRealDurationMs)}`;
  }
  return `×${fmt(state.visualizeSpeed, 2)}`;
}

function visualizeQueryParamsForSelection(selection) {
  return new URLSearchParams({
    swap_profile: selection.swap_profile,
    case_id: selection.scenario,
    algo: selection.algo,
    gps_layout: selection.gps_layout,
    graph_storage: selection.graph_storage,
    neighbor_access: selection.neighbor_access,
    exploration_policy: selection.exploration_policy,
    bucket_state: selection.bucket_state || "",
    heuristic: selection.heuristic,
  });
}

function visualizeQueryParams() {
  return visualizeQueryParamsForSelection(state.benchSelection);
}

function visualizeSelectionKey() {
  return visualizeQueryParams().toString();
}

function saveVisualizeSpatialState() {
  state.visualizeSpatialState = {
    liveSettled: state.liveSettled.slice(),
    liveCurrent: state.liveCurrent,
    liveRunning: state.liveRunning,
    path: state.path.slice(),
    cacheZones: Array.from(state.visualizeCacheZones.entries()),
    totalCacheHits: state.visualizeTotalCacheHits,
    totalCacheMisses: state.visualizeTotalCacheMisses,
    maxZoneAccesses: state.visualizeMaxZoneAccesses,
    openSet: Array.from(state.visualizeOpenSet.entries()),
    openSetTiles: Array.from(state.visualizeOpenSetTiles.entries()),
  };
}

function restoreVisualizeSpatialState() {
  const spatial = state.visualizeSpatialState;
  if (!spatial) return;
  state.liveSettled = spatial.liveSettled.slice();
  state.liveCurrent = spatial.liveCurrent;
  state.liveRunning = spatial.liveRunning;
  state.path = spatial.path.slice();
  state.visualizeCacheZones = new Map(spatial.cacheZones || []);
  state.visualizeTotalCacheHits = Number(spatial.totalCacheHits || 0);
  state.visualizeTotalCacheMisses = Number(spatial.totalCacheMisses || 0);
  state.visualizeMaxZoneAccesses = Number(spatial.maxZoneAccesses || 0);
  state.visualizeOpenSet = new Map(spatial.openSet || []);
  state.visualizeOpenSetTiles = new Map(spatial.openSetTiles || []);
}

function setVisualizeSpatialMode(mode) {
  state.visualizeSpatialMode = mode === "open_set" ? "open_set" : "locality";
  renderVisualizeSpatialControls();
  renderVisualizeMissLegend();
  draw();
}

function renderVisualizeSpatialControls() {
  const root = byId("visualize-spatial-controls");
  if (!root) return;
  root.hidden = state.mode !== "visualize";
  root.querySelectorAll("[data-spatial-mode]").forEach((button) => {
    const mode = button.dataset.spatialMode;
    button.classList.toggle("active", state.visualizeSpatialMode === mode);
  });
  const status = byId("visualize-spatial-status");
  if (status) status.textContent = "";
}

function setVisualizeStatus(message, status = "") {
  const el = byId("visualize-status");
  if (!el) return;
  el.className = "bench-status" + (status ? ` ${status}` : "");
  el.textContent = message;
}

function renderVisualizeRunMeta() {
  const root = byId("visualize-run-meta");
  if (!root) return;
  const run = state.visualizeRun;
  if (!run) {
    root.innerHTML = "";
    return;
  }
  const realDuration = state.visualizeRealDurationMs != null
    ? `<div>durée de recherche perf ${escapeHtml(formatReplayDuration(state.visualizeRealDurationMs))}${state.visualizeRealDurationSource ? ` · ${escapeHtml(state.visualizeRealDurationSource)}` : ""}</div>`
    : "";

  root.innerHTML = `
      <div><strong>${escapeHtml(run.run_id || "exécution instrumentée")}</strong></div>
      <div>${fmt(run.events_count || state.visualizeEvents.length)} événements · format v${escapeHtml(String(run.events_version || "1"))} · stride ${fmt(run.inspect_stride || 1)}</div>
      <div>${escapeHtml(run.timestamp || "")}</div>
      ${realDuration}
    `;
}

function renderVisualizeMissLegend() {
  const root = byId("visualize-miss-legend");
  if (!root) return;
  root.hidden = state.mode !== "visualize";

  const hits = state.visualizeTotalCacheHits;
  const misses = state.visualizeTotalCacheMisses;
  const accesses = hits + misses;
  const missRate = accesses > 0 ? (misses / accesses) * 100 : 0;

  if (state.visualizeSpatialMode === "open_set") {
    const openSetEntries = Array.from(state.visualizeOpenSetTiles.values())
      .reduce((sum, zone) => sum + Number(zone.count || 0), 0);
    root.innerHTML = `
        <strong>Frontière</strong>
        <span><i class="miss-low"></i> 0 %</span>
        <span><i class="miss-mid"></i> 1 %</span>
        <span><i class="miss-high"></i> ≥ 2 %</span>
        <small>couleur = taux de miss · taille = entrées courantes · alpha = pic historique · ${fmt(openSetEntries)} entrées</small>
      `;
    return;
  }
  root.innerHTML = `
      <strong>Cache</strong>
      <span><i class="miss-low"></i> 0 %</span>
      <span><i class="miss-mid"></i> 1 %</span>
      <span><i class="miss-high"></i> ≥ 2 %</span>
      <small>couleur = taux de miss · taille = accès cumulés · alpha = activité récente · ${fmt(misses)} / ${fmt(accesses)} accès · global ${fmt(missRate, 2)} %</small>
    `;
}

function renderVisualizeProgress() {
  const total = state.visualizeEvents.length;
  const progress = byId("visualize-progress");
  const label = byId("visualize-progress-label");
  if (progress) {
    progress.max = Math.max(1, total);
    progress.value = Math.min(state.visualizeCursor, total);
  }
  if (label) label.textContent = `${fmt(state.visualizeCursor)} / ${fmt(total)}`;

  const current = byId("visualize-current-event");
  if (current) {
    const event = state.visualizeCurrentEvent;
    current.textContent = event
      ? `#${event.seq ?? state.visualizeCursor - 1} · ${event.type}${event.collection ? ` · ${event.collection}.${event.action}` : ""}`
      : "Aucun événement rejoué.";
  }
  renderVisualizeMissLegend();
  renderVisualizeSpatialControls();
}

function resetVisualizeReplay() {
  pauseVisualizeReplay();
  state.visualizeCursor = 0;
  state.visualizeCarry = 0;
  state.visualizeLastFrameAt = 0;
  state.visualizeCurrentEvent = null;
  state.visualizeCollections = emptyCollectionReplayState();
  state.visualizeCollectionPressure = emptyCollectionPressureState();
  resetVisualizeCacheTimeline();
  state.visualizeRamState = null;
  state.visualizeCacheZones = new Map();
  state.visualizeTotalCacheHits = 0;
  state.visualizeTotalCacheMisses = 0;
  state.visualizeCacheSourceTotals = {};
  state.visualizeMaxZoneAccesses = 0;
  state.visualizeOpenSet = new Map();
  state.visualizeOpenSetTiles = new Map();
  resetLive();
  saveVisualizeSpatialState();
  renderVisualizeProgress();
  renderVisualizeRam();
  renderVisualizeCollections();
  renderVisualizeCacheTimeline();
  draw();
}

function pauseVisualizeReplay() {
  state.visualizePlaying = false;
  if (state.visualizeFrame != null) {
    cancelAnimationFrame(state.visualizeFrame);
    state.visualizeFrame = null;
  }
}

function applyVisualizeEvent(event) {
  state.visualizeCurrentEvent = event;

  if (isCacheNodeEvent(event)) {
    const delta = cacheNodeDelta(event);
    const hits = Number(delta.cache_hits || 0);
    const misses = Number(delta.cache_misses || 0);
    const accesses = hits + misses;

    recordVisualizeCacheSourceDelta(event.source, delta);

    if (accesses > 0) {
      const { tileX, tileY } = tileCoordinates(event.node);
      const key = tileKey(tileX, tileY);
      const zone = state.visualizeCacheZones.get(key) || {
        tileX,
        tileY,
        hits: 0,
        misses: 0,
        accesses: 0,
        recentActivity: 0,
        lastSeq: Number(event.seq || 0),
      };

      zone.hits += hits;
      zone.misses += misses;
      zone.accesses += accesses;
      updateCacheActivity(zone, event.seq, accesses, state.visualizeEvents.length);
      state.visualizeCacheZones.set(key, zone);
      state.visualizeTotalCacheHits += hits;
      state.visualizeTotalCacheMisses += misses;
      state.visualizeMaxZoneAccesses = Math.max(
        state.visualizeMaxZoneAccesses,
        zone.accesses,
      );
    }
    recordVisualizeCacheTimeline(event, hits, misses);
    return;
  }

  if (event.type === "collection") {
    if (event.ram_state) state.visualizeRamState = event.ram_state;
    const current = state.visualizeCollections[event.collection];
    if (!current) return;
    current.len = Number(event.len_after || 0);
    current.action = String(event.action || "—");
    current.changed = Boolean(event.changed);

    const source = COLLECTION_CACHE_SOURCES[event.collection];
    const totals = source ? state.visualizeCacheSourceTotals[source] : null;
    if (totals) {
      current.delta = copyCacheDelta(totals.delta);
      current.deltaAvailable = totals.events > 0;
    } else {
      const legacyDelta = event.cache_delta || {};
      const hasLegacyDelta = CACHE_DELTA_KEYS.some((key) => Number(legacyDelta[key] || 0) !== 0);
      current.delta = hasLegacyDelta ? copyCacheDelta(legacyDelta) : emptyCacheDelta();
      current.deltaAvailable = hasLegacyDelta;
    }

    if (event.cache_state) {
      current.cacheState = event.cache_state;
      recordCollectionPressure(
        event.collection,
        event.cache_state,
        event.ram_state || state.visualizeRamState,
        event.seq,
      );
    }
    return;
  }

  if (event.type === "started") {
    resetLive();
    state.liveRunning = true;
    return;
  }
  if (event.type === "pushed" && event.node) {
    addOpenSetEntry(event);
    return;
  }
  if (event.type === "popped") {
    removeOpenSetEntry(event);
    state.liveCurrent = event.node || null;
    return;
  }
  if (event.type === "settled" && event.node) {
    state.liveSettled.push(event.node);
    if (state.liveSettled.length > 20000) state.liveSettled.shift();
    return;
  }
  if (event.type === "relaxed") return;
  if (event.type === "path_node" && event.node) {
    state.path.push(event.node);
    return;
  }
  if (event.type === "finished") {
    state.liveRunning = false;
    state.liveCurrent = null;
    if (state.visualizeCacheTimelineAvailable) {
      state.visualizeCacheTimelineCurrentTick = state.visualizeCacheTimelineMaxTick;
    }
  }
}

function visualizeReplayFrame(now) {
  if (!state.visualizePlaying) return;

  if (!state.visualizeLastFrameAt) state.visualizeLastFrameAt = now;
  const elapsedSeconds = Math.max(0, now - state.visualizeLastFrameAt) / 1000;
  state.visualizeLastFrameAt = now;
  state.visualizeCarry += elapsedSeconds * visualizeEventsPerSecond();

  const remaining = state.visualizeEvents.length - state.visualizeCursor;
  const count = Math.min(remaining, Math.floor(state.visualizeCarry));
  if (count > 0) state.visualizeCarry -= count;

  for (let index = 0; index < count; index += 1) {
    applyVisualizeEvent(state.visualizeEvents[state.visualizeCursor]);
    state.visualizeCursor += 1;
  }

  if (count > 0) {
    renderVisualizeProgress();
    renderVisualizeRam();
    renderVisualizeCollections();
    renderVisualizeCacheTimeline();
    draw();
  }

  if (state.visualizeCursor >= state.visualizeEvents.length) {
    pauseVisualizeReplay();
    setVisualizeStatus("Replay terminé.", "ok");
    return;
  }

  state.visualizeFrame = requestAnimationFrame(visualizeReplayFrame);
}

function playVisualizeReplay() {
  if (!state.visualizeEvents.length) {
    setVisualizeStatus("Aucun événement instrumenté chargé.", "no-data");
    return;
  }
  if (state.visualizeCursor >= state.visualizeEvents.length) resetVisualizeReplay();
  if (state.visualizePlaying) return;
  state.visualizePlaying = true;
  state.visualizeLastFrameAt = 0;
  setVisualizeStatus(`Replay en cours · ${visualizeSpeedLabel()}.`, "ok");
  state.visualizeFrame = requestAnimationFrame(visualizeReplayFrame);
}

function parseNdjson(text) {
  const events = [];
  let expectedSeq = 0;
  for (const [index, line] of text.split(/\r?\n/).entries()) {
    if (!line.trim()) continue;
    const event = JSON.parse(line);
    const version = String(event.v);
    if (!["1", "2", "3", "4", "5", "2.0", "2.1", "2.2", "2.3", "2.4"].includes(version)) throw new Error(`Version d'événement inconnue à la ligne ${index + 1}.`);
    if (Number(event.seq) !== expectedSeq) throw new Error(`Rupture de séquence : attendu ${expectedSeq}, reçu ${event.seq}.`);
    expectedSeq += 1;
    events.push(event);
  }
  return events;
}

function showVisualizeNoData(response) {
  pauseVisualizeReplay();
  state.visualizeRun = null;
  state.visualizeEvents = [];
  state.visualizeCacheTimelineAvailable = false;
  state.visualizeCacheTimelineMaxTick = 0;
  state.visualizeCacheTimelineSources = [];
  state.visualizeRealDurationMs = null;
  state.visualizeRealDurationSource = "";
  updateVisualizeSpeedControl();
  resetVisualizeReplay();
  setVisualizeStatus(response.message || "Aucun replay instrumenté disponible.", "no-data");
  renderVisualizeRunMeta();
  const error = byId("visualize-error-json");
  if (error) {
    error.hidden = false;
    error.textContent = JSON.stringify(response, null, 2);
  }
}

function fetchVisualizeResult() {
  if (!state.benchConfig || state.mode !== "visualize") return;

  const key = visualizeSelectionKey();
  if (state.visualizeSelectionKey === key && state.visualizeRun && state.visualizeEvents.length) {
    restoreVisualizeSpatialState();
    renderVisualizeRunMeta();
    renderVisualizeProgress();
    renderVisualizeRam();
    renderVisualizeCollections();
    setVisualizeStatus("Replay conservé en pause.", "ok");
    draw();
    return;
  }

  pauseVisualizeReplay();
  state.visualizeSelectionKey = key;
  state.visualizeSpatialState = null;
  setVisualizeStatus("Recherche du replay instrumenté…");
  const error = byId("visualize-error-json");
  if (error) {
    error.hidden = true;
    error.textContent = "";
  }

  const params = visualizeQueryParams();
  fetchJson("/api/visualize/result?" + params.toString())
    .then((response) => {
      if (response.status !== "ok" || !response.run) {
        showVisualizeNoData(response);
        return null;
      }

      state.visualizeRun = response.run;
      fetchVisualizeRealDuration(params, key);
      renderVisualizeRunMeta();
      setVisualizeStatus("Chargement des événements instrumentés…");

      return fetch("/api/visualize/events?" + params.toString()).then(async (httpResponse) => {
        const text = await httpResponse.text();
        if (!httpResponse.ok) {
          let payload;
          try { payload = JSON.parse(text); } catch (_) { payload = { status: "error", message: text || httpResponse.statusText }; }
          throw new Error(JSON.stringify(payload));
        }
        return text;
      });
    })
    .then((text) => {
      if (text == null) return;
      state.visualizeEvents = parseNdjson(text);
      configureVisualizeCacheTimeline(state.visualizeEvents);
      const references = spatialReplayReferences(state.visualizeEvents);
      state.visualizeZoneAccessReference = references.zoneAccessReference;
      state.visualizeOpenSetPeakReference = references.openSetPeakReference;
      resetVisualizeReplay();
      state.visualizeSelectionKey = key;
      setVisualizeStatus(`${fmt(state.visualizeEvents.length)} événements prêts.`, "ok");
      renderVisualizeRunMeta();
    })
    .catch((errorValue) => {
      const response = { status: "error", message: errorValue.message || String(errorValue) };
      showVisualizeNoData(response);
    });
}

function installVisualizeHandlers() {
  const on = (id, event, callback) => {
    const el = byId(id);
    if (el) el.addEventListener(event, callback);
  };
  on("visualize-play", "click", playVisualizeReplay);
  on("visualize-toggle-left", "click", () => toggleVisualizeSide("left"));
  on("visualize-toggle-right", "click", () => toggleVisualizeSide("right"));
  const cacheTimeline = byId("visualize-cache-timeline");
  if (cacheTimeline) {
    cacheTimeline.addEventListener("click", (event) => {
      const metricButton = event.target.closest("[data-cache-timeline-mode]");
      if (metricButton) {
        state.visualizeCacheTimelineMode = metricButton.dataset.cacheTimelineMode === "rate"
          ? "rate"
          : "misses";
        renderVisualizeCacheTimeline();
        return;
      }

      const layoutButton = event.target.closest("[data-cache-timeline-layout]");
      if (!layoutButton) return;
      state.visualizeCacheTimelineLayout = layoutButton.dataset.cacheTimelineLayout === "split"
        ? "split"
        : "overlay";
      renderVisualizeCacheTimeline();
    });
  }
  const spatialControls = byId("visualize-spatial-controls");
  if (spatialControls) {
    spatialControls.addEventListener("click", (event) => {
      const button = event.target.closest("[data-spatial-mode]");
      if (!button || button.disabled) return;
      setVisualizeSpatialMode(button.dataset.spatialMode);
    });
  }
  on("visualize-pause", "click", () => {
    pauseVisualizeReplay();
    setVisualizeStatus("Replay en pause.");
  });
  on("visualize-reset", "click", () => {
    resetVisualizeReplay();
    setVisualizeStatus(state.visualizeEvents.length ? "Replay remis au début." : "Aucun replay chargé.");
  });
  on("visualize-speed", "change", (event) => {
    if (event.target.value === "real") {
      if (state.visualizeRealDurationMs == null) {
        event.target.value = "1";
        state.visualizeSpeedMode = "multiplier";
        state.visualizeSpeed = 1;
        setVisualizeStatus("Durée instrumentée indisponible pour cette sélection.", "no-data");
        return;
      }
      state.visualizeSpeedMode = "real";
    } else {
      const speed = Number(event.target.value);
      state.visualizeSpeedMode = "multiplier";
      state.visualizeSpeed = Number.isFinite(speed) && speed > 0 ? speed : 1;
    }

    state.visualizeCarry = 0;
    state.visualizeLastFrameAt = 0;
    if (state.visualizePlaying) {
      setVisualizeStatus(`Replay en cours · ${visualizeSpeedLabel()}.`, "ok");
    }
  });
  updateVisualizeSpeedControl();
  renderVisualizeSpatialControls();
  renderVisualizeLayoutControls();
}

export {
  fetchVisualizeResult,
  formatReplayDuration,
  installVisualizeHandlers,
  pauseVisualizeReplay,
  renderVisualizeSpatialControls,
  saveVisualizeSpatialState,
};
