// Carte, dessin et affichage spatial du replay

import { byId, debug, setStatus } from "./utils.js";
import {
  CACHE_TILE_MIN_SCALE,
  LOCALITY_RED_RATE,
  TILE_ACTIVITY_DECAY_FRACTION,
  TILE_ACTIVITY_DECAY_MIN_EVENTS,
  TILE_ACTIVITY_GLOW_SCALE,
  TILE_ACTIVITY_MAX_ALPHA,
  TILE_ACTIVITY_MAX_SCALE,
  TILE_ACTIVITY_MIN_ALPHA,
  TILE_ACTIVITY_MIN_SCALE,
  TILE_UNITS,
  VISUALIZE_MAX_BACKGROUND_NODES,
  log,
  state,
} from "./state.js";
import { benchAlgorithmDescriptor, currentScenario } from "./controls.js";

function browserPlatform() {
  const canvas = byId("map");

  if (!canvas) {
    throw new Error("canvas #map introuvable");
  }

  const ctx = canvas.getContext("2d");

  function size() {
    const rect = canvas.getBoundingClientRect();

    return {
      width: Math.max(1, rect.width),
      height: Math.max(1, rect.height),
    };
  }

  function resize() {
    const rect = canvas.getBoundingClientRect();
    const ratio = window.devicePixelRatio || 1;

    canvas.width = Math.max(1, Math.floor(rect.width * ratio));
    canvas.height = Math.max(1, Math.floor(rect.height * ratio));

    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
  }

  function clear() {
    resize();

    const s = size();

    ctx.clearRect(0, 0, s.width, s.height);
    ctx.fillStyle = "#0f1218";
    ctx.fillRect(0, 0, s.width, s.height);
  }

  function line(a, b, style) {
    ctx.beginPath();
    ctx.moveTo(a.x, a.y);
    ctx.lineTo(b.x, b.y);

    ctx.lineWidth = style.width || 1;
    ctx.strokeStyle = style.color || "#ffffff";
    ctx.globalAlpha = style.alpha == null ? 1 : style.alpha;

    ctx.stroke();

    ctx.globalAlpha = 1;
  }

  function circle(p, r, style) {
    ctx.beginPath();
    ctx.arc(p.x, p.y, r, 0, Math.PI * 2);

    ctx.fillStyle = style.fill || "#ffffff";
    ctx.strokeStyle = style.stroke || style.fill || "#ffffff";
    ctx.lineWidth = style.width || 1;
    ctx.globalAlpha = style.alpha == null ? 1 : style.alpha;

    ctx.fill();

    if (style.stroke) {
      ctx.stroke();
    }

    ctx.globalAlpha = 1;
  }

  function rect(a, b, style) {
    const x = Math.min(a.x, b.x);
    const y = Math.min(a.y, b.y);
    const width = Math.abs(b.x - a.x);
    const height = Math.abs(b.y - a.y);

    ctx.save();
    ctx.globalCompositeOperation = style.composite || "source-over";
    ctx.fillStyle = style.fill || "#ffffff";
    ctx.globalAlpha = style.alpha == null ? 1 : style.alpha;
    ctx.fillRect(x, y, width, height);

    if (style.stroke) {
      ctx.strokeStyle = style.stroke;
      ctx.lineWidth = style.width || 1;
      ctx.strokeRect(x, y, width, height);
    }

    ctx.restore();
  }

  function text(value, p, style) {
    ctx.font = style.font || "12px system-ui";
    ctx.fillStyle = style.color || "#e8eef8";
    ctx.fillText(value, p.x, p.y);
  }

  function fetchSnapshot(callback) {
    fetch("/api/snapshot")
      .then((r) => {
        log.http("GET", "/api/snapshot", r.status);
        if (!r.ok) throw new Error("/api/snapshot HTTP " + r.status);
        return r.json();
      })
      .then(callback)
      .catch((e) => {
        console.error(e);
        const subtitle = byId("subtitle");
        if (subtitle) subtitle.textContent = "Erreur de chargement /api/snapshot";
        setStatus("Erreur /api/snapshot : " + e.message);
      });
  }

  function snapNode(lon, lat, label, role, callback) {
    const params = new URLSearchParams({
      lon: String(lon),
      lat: String(lat),
      label,
      role,
    });

    log.snapRequest(lon, lat, label, role);

    fetch("/api/snap?" + params.toString())
      .then(async (r) => {
        if (!r.ok) {
          const text = await r.text();
          throw new Error(text || r.statusText);
        }
        return r.json();
      })
      .then((node) => {
        log.snapResponse(node);
        callback(node);
      })
      .catch((e) => {
        log.snapError(e);
        setStatus("Erreur snap : " + e.message);
      });
  }

  function installInput(callbacks) {
    canvas.addEventListener("mousedown", (e) => {
      state.dragging = true;
      canvas.classList.add("dragging");
      state.lastX = e.clientX;
      state.lastY = e.clientY;
    });

    window.addEventListener("mouseup", () => {
      state.dragging = false;
      canvas.classList.remove("dragging");
    });

    window.addEventListener("mousemove", (e) => {
      if (!state.dragging) return;
      const dx = e.clientX - state.lastX;
      const dy = e.clientY - state.lastY;
      state.lastX = e.clientX;
      state.lastY = e.clientY;
      callbacks.pan(dx, dy);
    });

    canvas.addEventListener(
      "wheel",
      (e) => {
        e.preventDefault();
        callbacks.zoom(e.deltaY < 0 ? 1.16 : 1 / 1.16);
      },
      { passive: false },
    );

    window.addEventListener("resize", callbacks.redraw);
  }


  return {
    size,
    clear,
    line,
    circle,
    rect,
    text,
    fetchSnapshot,
    snapNode,
    installInput,
  };
}

const platform = browserPlatform();

function resetCamera() {
  const snap = state.snapshot;
  const size = platform.size();

  state.centerX = snap.viewport.width / 2;
  state.centerY = snap.viewport.height / 2;

  state.scale = Math.min(
    (size.width * 0.92) / snap.viewport.width,
    (size.height * 0.92) / snap.viewport.height
  );

  log.camera({
    centerX: state.centerX,
    centerY: state.centerY,
    scale: state.scale,
    canvas: size,
  });
}

function worldToScreen(x, y) {
  const size = platform.size();

  return {
    x: size.width / 2 + (x - state.centerX) * state.scale,
    y: size.height / 2 - (y - state.centerY) * state.scale,
  };
}

function pan(dx, dy) {
  state.centerX -= dx / state.scale;
  state.centerY += dy / state.scale;
  draw();
}

function zoom(factor) {
  state.scale = Math.max(0.002, Math.min(200, state.scale * factor));
  draw();
}

function drawGrid() {
  const snap = state.snapshot;
  for (let x = 0; x <= snap.viewport.width; x += TILE_UNITS) {
    platform.line(worldToScreen(x, 0), worldToScreen(x, snap.viewport.height), {
      color: "#334155",
      width: 0.45,
      alpha: 0.35,
    });
  }
  for (let y = 0; y <= snap.viewport.height; y += TILE_UNITS) {
    platform.line(worldToScreen(0, y), worldToScreen(snap.viewport.width, y), {
      color: "#334155",
      width: 0.45,
      alpha: 0.35,
    });
  }
}

function drawSegments() {
  const segments = state.snapshot.layers.segments || [];
  log.renderEvery("drawSegments", { segments: segments.length }, 1500);
  for (const segment of segments) {
    platform.line(
      worldToScreen(segment.x1, segment.y1),
      worldToScreen(segment.x2, segment.y2),
      { color: "#475569", width: 0.55, alpha: 0.46 },
    );
  }
}

function drawNodes() {
  for (const node of state.nodes) {
    const p = worldToScreen(node.x, node.y);
    const isStart = node.role === "start";

    platform.circle(p, 7, {
      fill: isStart ? "#4ade80" : "#fb7185",
      stroke: "#0f1218",
      width: 2,
    });

    platform.text(node.label, { x: p.x + 10, y: p.y - 10 }, { color: "#e8eef8" });
  }
}

function drawPath() {
  if (!state.path || state.path.length < 2) return;

  for (let i = 0; i + 1 < state.path.length; i++) {
    platform.line(
      worldToScreen(state.path[i].x, state.path[i].y),
      worldToScreen(state.path[i + 1].x, state.path[i + 1].y),
      { color: "#fb7185", width: 3 }
    );
  }
}

function drawHud() {
  const hud = byId("hud");
  if (!hud) return;
  const scenario = currentScenario();
  hud.textContent =
    (scenario ? scenario.name : "bench") +
    " · " +
    (benchAlgorithmDescriptor(state.benchSelection.algo)?.label || state.benchSelection.algo) +
    " · " +
    state.benchSelection.gps_layout;
}

function draw() {
  if (!state.snapshot) {
    debug("draw skipped: no snapshot");
    return;
  }

  platform.clear();
  drawGrid();
  drawSegments();

  drawLiveSettled();
  drawVisualizeSpatialMode();
  drawPath();
  drawLiveCurrent();

  drawNodes();
  drawHud();
}

function resetLive() {
  state.liveSettled = [];
  state.liveCurrent = null;
  state.path = [];
}

function drawLiveSettled() {
  const size = platform.size();
  const nodes = state.liveSettled;
  const step = state.mode === "visualize"
    ? Math.max(1, Math.ceil(nodes.length / VISUALIZE_MAX_BACKGROUND_NODES))
    : 1;

  for (let index = 0; index < nodes.length; index += step) {
    const node = nodes[index];
    const p = worldToScreen(node.x, node.y);

    if (p.x < -10 || p.y < -10 || p.x > size.width + 10 || p.y > size.height + 10) {
      continue;
    }

    platform.circle(p, state.mode === "visualize" ? 1.25 : 1.8, {
      fill: state.mode === "visualize" ? "#a5f3fc" : "#22c55e",
      alpha: state.mode === "visualize" ? 0.25 : 0.45,
    });
  }
}

function nodeReplayKey(node) {
  if (!node) return "";
  return `${Number(node.x || 0)}:${Number(node.y || 0)}`;
}

function isCacheNodeEvent(event) {
  return (event?.type === "cache_node" || event?.type === "node_cache") && event.node;
}

function cacheNodeDelta(event) {
  return event?.delta || event?.cache_delta || {};
}

function openSetEntryKey(event) {
  return `${nodeReplayKey(event.node)}:${Number(event.cost || 0)}:${Number(event.estimation || 0)}`;
}

function tileCoordinates(node) {
  return {
    tileX: Number(node?.tile_x ?? Math.floor(Number(node?.x || 0) / TILE_UNITS)),
    tileY: Number(node?.tile_y ?? Math.floor(Number(node?.y || 0) / TILE_UNITS)),
  };
}

function tileKey(tileX, tileY) {
  return `${tileX}:${tileY}`;
}

function tileScreenBox(tileX, tileY, scale = 1, offsetX = 0, offsetY = 0) {
  const start = worldToScreen(tileX * TILE_UNITS, tileY * TILE_UNITS);
  const end = worldToScreen((tileX + 1) * TILE_UNITS, (tileY + 1) * TILE_UNITS);
  const left = Math.min(start.x, end.x);
  const right = Math.max(start.x, end.x);
  const top = Math.min(start.y, end.y);
  const bottom = Math.max(start.y, end.y);
  const width = right - left;
  const height = bottom - top;
  const scaledWidth = width * scale;
  const scaledHeight = height * scale;
  const centerX = (left + right) / 2 + width * offsetX;
  const centerY = (top + bottom) / 2 + height * offsetY;

  return {
    start: { x: centerX - scaledWidth / 2, y: centerY - scaledHeight / 2 },
    end: { x: centerX + scaledWidth / 2, y: centerY + scaledHeight / 2 },
    center: { x: centerX, y: centerY },
    width,
    height,
  };
}

function activityReference(values) {
  let minimum = Infinity;
  let maximum = 0;

  for (const rawValue of values) {
    const value = Number(rawValue || 0);
    if (!Number.isFinite(value) || value <= 0) continue;
    minimum = Math.min(minimum, value);
    maximum = Math.max(maximum, value);
  }

  if (!Number.isFinite(minimum) || maximum <= 0) {
    return { minimum: 1, maximum: 1 };
  }

  return { minimum, maximum };
}

function spatialReplayReferences(events) {
  const cacheAccesses = new Map();
  const openSet = new Map();
  const openSetTiles = new Map();
  for (const event of events) {
    if (isCacheNodeEvent(event)) {
      const delta = cacheNodeDelta(event);
      const accesses = Number(delta.cache_hits || 0) + Number(delta.cache_misses || 0);
      if (accesses > 0) {
        const { tileX, tileY } = tileCoordinates(event.node);
        const key = tileKey(tileX, tileY);
        cacheAccesses.set(key, Number(cacheAccesses.get(key) || 0) + accesses);
      }
      continue;
    }

    if ((event.type !== "pushed" && event.type !== "popped") || !event.node) continue;

    const entryKey = openSetEntryKey(event);
    const { tileX, tileY } = tileCoordinates(event.node);
    const zoneKey = tileKey(tileX, tileY);
    const zone = openSetTiles.get(zoneKey) || { count: 0, peak: 0 };

    if (event.type === "pushed") {
      openSet.set(entryKey, Number(openSet.get(entryKey) || 0) + 1);
      zone.count += 1;
      zone.peak = Math.max(zone.peak, zone.count);
      openSetTiles.set(zoneKey, zone);
      continue;
    }

    const entryCount = Number(openSet.get(entryKey) || 0);
    if (entryCount <= 0) continue;
    if (entryCount === 1) openSet.delete(entryKey);
    else openSet.set(entryKey, entryCount - 1);
    zone.count = Math.max(0, zone.count - 1);
    openSetTiles.set(zoneKey, zone);
  }

  return {
    zoneAccessReference: activityReference(cacheAccesses.values()),
    openSetPeakReference: activityReference(
      Array.from(openSetTiles.values(), (zone) => zone.peak),
    ),
  };
}

function activityRatio(value, reference) {
  const current = Number(value || 0);
  if (!Number.isFinite(current) || current <= 0) return 0;

  const minimum = Math.max(0, Number(reference?.minimum || 0));
  const maximum = Math.max(minimum, Number(reference?.maximum || minimum));
  if (maximum <= minimum) return 1;

  const low = Math.log1p(minimum);
  const high = Math.log1p(maximum);
  const ratio = (Math.log1p(current) - low) / Math.max(1e-9, high - low);
  return Math.max(0, Math.min(1, ratio));
}

function activityScale(
  value,
  reference,
  maximumScale = TILE_ACTIVITY_MAX_SCALE,
  minimumScale = TILE_ACTIVITY_MIN_SCALE,
) {
  if (Number(value || 0) <= 0) return 0;
  const ratio = activityRatio(value, reference);
  return minimumScale + (maximumScale - minimumScale) * ratio;
}

function cacheTileScale(
  accesses,
  reference,
  maximumScale = TILE_ACTIVITY_MAX_SCALE,
  minimumScale = CACHE_TILE_MIN_SCALE,
) {
  const current = Number(accesses || 0);
  if (!Number.isFinite(current) || current <= 0) return 0;

  const maximum = Math.max(1, Number(reference?.maximum || 1));
  const ratio = Math.max(0, Math.min(1, current / maximum));
  return Math.max(minimumScale, maximumScale * Math.sqrt(ratio));
}

function activityAlpha(value, reference) {
  const ratio = activityRatio(value, reference);
  return TILE_ACTIVITY_MIN_ALPHA +
    ratio * (TILE_ACTIVITY_MAX_ALPHA - TILE_ACTIVITY_MIN_ALPHA);
}

function cacheActivityTau(totalEvents) {
  return Math.max(
    TILE_ACTIVITY_DECAY_MIN_EVENTS,
    Number(totalEvents || 0) * TILE_ACTIVITY_DECAY_FRACTION,
  );
}

function decayedCacheActivity(zone, currentSeq, totalEvents) {
  const activity = Math.max(0, Number(zone?.recentActivity || 0));
  const lastSeq = Math.max(0, Number(zone?.lastSeq || 0));
  const age = Math.max(0, Number(currentSeq || 0) - lastSeq);
  return activity * Math.exp(-age / cacheActivityTau(totalEvents));
}

function updateCacheActivity(zone, eventSeq, accesses, totalEvents) {
  const seq = Math.max(0, Number(eventSeq || 0));
  zone.recentActivity = decayedCacheActivity(zone, seq, totalEvents) + accesses;
  zone.lastSeq = seq;
}

function cacheActivityAlpha(zone, currentSeq, totalEvents, reference) {
  const recent = decayedCacheActivity(zone, currentSeq, totalEvents);
  const ratio = activityRatio(recent, reference);
  const age = Math.max(0, Number(currentSeq || 0) - Number(zone?.lastSeq || 0));
  const freshness = Math.exp(-age / cacheActivityTau(totalEvents));
  return 0.08 + 0.74 * Math.sqrt(Math.max(0, ratio * freshness));
}

function drawActivityTile(tileX, tileY, scale, color, alpha, intensityRatio) {
  if (scale <= 0) return;

  const glowScale = Math.min(1, scale + TILE_ACTIVITY_GLOW_SCALE);
  const glowBox = tileScreenBox(tileX, tileY, glowScale);
  platform.rect(glowBox.start, glowBox.end, {
    fill: color,
    alpha: 0.035 + Math.max(0, Math.min(1, intensityRatio)) * 0.08,
    composite: "lighter",
  });

  const box = tileScreenBox(tileX, tileY, scale);
  platform.rect(box.start, box.end, {
    fill: color,
    alpha,
  });
}

function updateOpenSetTile(node, delta) {
  if (!node || !delta) return;
  const { tileX, tileY } = tileCoordinates(node);
  const key = tileKey(tileX, tileY);
  const zone = state.visualizeOpenSetTiles.get(key) || {
    key,
    tileX,
    tileY,
    count: 0,
    peak: 0,
  };

  zone.count = Math.max(0, Number(zone.count || 0) + delta);
  zone.peak = Math.max(Number(zone.peak || 0), zone.count);
  state.visualizeOpenSetTiles.set(key, zone);
}

function addOpenSetEntry(event) {
  const key = openSetEntryKey(event);
  const existing = state.visualizeOpenSet.get(key);
  if (existing) {
    existing.count += 1;
  } else {
    state.visualizeOpenSet.set(key, { node: event.node, count: 1 });
  }
  updateOpenSetTile(event.node, 1);
}

function removeOpenSetEntry(event) {
  const key = openSetEntryKey(event);
  const existing = state.visualizeOpenSet.get(key);
  if (!existing) return;
  existing.count -= 1;
  updateOpenSetTile(event.node, -1);
  if (existing.count <= 0) state.visualizeOpenSet.delete(key);
}

function localityColor(missRate) {
  const severity = Math.max(0, Math.min(1, missRate / LOCALITY_RED_RATE));
  const hue = Math.round(120 * (1 - severity));
  return `hsl(${hue} 92% 48%)`;
}

function drawVisualizeLocalityZones() {
  if (state.mode !== "visualize" || !state.visualizeCacheZones.size) return;
  const currentSeq = Number(state.visualizeCurrentEvent?.seq ?? state.visualizeCursor ?? 0);
  const totalEvents = state.visualizeEvents.length;
  const currentReference = state.visualizeZoneAccessReference;
  const alphaReference = activityReference(
    Array.from(
      state.visualizeCacheZones.values(),
      (zone) => decayedCacheActivity(zone, currentSeq, totalEvents),
    ),
  );

  for (const zone of state.visualizeCacheZones.values()) {
    if (!zone.accesses) continue;

    const missRate = zone.misses / zone.accesses;
    const color = localityColor(missRate);
    const scale = cacheTileScale(zone.accesses, currentReference);
    const recentActivity = decayedCacheActivity(zone, currentSeq, totalEvents);
    const alpha = cacheActivityAlpha(zone, currentSeq, totalEvents, alphaReference);
    const intensityRatio = activityRatio(recentActivity, alphaReference);

    drawActivityTile(zone.tileX, zone.tileY, scale, color, alpha, intensityRatio);
  }
}

function drawVisualizeOpenSetTiles() {
  if (!state.visualizeOpenSetTiles.size) return;
  const activeZones = Array.from(state.visualizeOpenSetTiles.values())
    .filter((zone) => Number(zone.count || 0) > 0);
  const currentReference = activityReference(activeZones.map((zone) => zone.count));
  const peakReference = state.visualizeOpenSetPeakReference;

  for (const zone of activeZones) {
    const cacheZone = state.visualizeCacheZones.get(zone.key);
    const missRate = cacheZone?.accesses
      ? Number(cacheZone.misses || 0) / Number(cacheZone.accesses || 1)
      : 0;
    const color = localityColor(missRate);
    const scale = activityScale(zone.count, currentReference);
    const alpha = activityAlpha(zone.peak, peakReference);
    const intensityRatio = activityRatio(zone.peak, peakReference);

    drawActivityTile(zone.tileX, zone.tileY, scale, color, alpha, intensityRatio);
  }
}

function drawVisualizeSpatialMode() {
  if (state.mode !== "visualize") return;
  if (state.visualizeSpatialMode === "open_set") drawVisualizeOpenSetTiles();
  else drawVisualizeLocalityZones();
}

function drawLiveCurrent() {
  if (!state.liveCurrent) return;

  const p = worldToScreen(state.liveCurrent.x, state.liveCurrent.y);

  platform.circle(p, 7, {
    fill: "#f8fafc",
    stroke: "#6d28d9",
    width: 2,
  });
}

function placeCoord(role, lon, lat, callback) {
  const label = role === "start" ? "Départ" : "Arrivée";

  platform.snapNode(lon, lat, label, role, (node) => {
    state.nodes = state.nodes.filter((n) => n.role !== role);
    state.nodes.push(node);
    state.path = [];
    log.pointPlaced(label, node);
    draw();
    if (callback) callback(node);
  });
}

export {
  addOpenSetEntry,
  cacheNodeDelta,
  draw,
  isCacheNodeEvent,
  pan,
  placeCoord,
  platform,
  removeOpenSetEntry,
  resetCamera,
  resetLive,
  spatialReplayReferences,
  tileCoordinates,
  tileKey,
  updateCacheActivity,
  zoom,
};
