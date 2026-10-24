// Chargement des mesures et cartes de statistiques.

import {
  FLASH_READ_LATENCY_MS,
  FLASH_WRITE_LATENCY_MS,
  HOST_REFERENCE_CPU_MHZ,
  RP2040_CPU_MHZ,
  THEME_DEFS,
  THEME_SUMMARY_METRICS,
  state,
} from "../state.js";
import { byId, clamp, escapeHtml, fetchJson, fmt, fmtBytes, fmtMetric } from "../utils.js";
import {
  baselineAlgoLabel,
  baselineComparisonTitle,
  modalKpiHtml,
  noDataBlock,
  openThemeDetails,
} from "./details.js";
import { metricOf } from "./metrics.js";
import { normalizeBenchAlgoId } from "../controls.js";
import { formatReplayDuration } from "../replay/player.js";

function fetchBenchResult() {
  if (!state.benchConfig || state.mode !== "stats") return;

  const requestId = ++state.benchRequestId;
  const params = new URLSearchParams({
    swap_profile: state.benchSelection.swap_profile,
    case_id: state.benchSelection.scenario,
    algo: state.benchSelection.algo,
    baseline_algo: state.benchSelection.baseline_algo,
    gps_layout: state.benchSelection.gps_layout,
    graph_storage: state.benchSelection.graph_storage,
    neighbor_access: state.benchSelection.neighbor_access,
    exploration_policy: state.benchSelection.exploration_policy,
    bucket_state: state.benchSelection.bucket_state || "",
    heuristic: state.benchSelection.heuristic,
  });

  const status = byId("bench-status");
  if (status) status.textContent = "Chargement métriques…";

  fetchJson("/api/bench/result?" + params.toString())
    .then((response) => {
      if (requestId !== state.benchRequestId || state.mode !== "stats") return;
      state.benchLastResponse = response;
      renderBenchMetrics(response);
    })
    .catch(() => {
      if (requestId !== state.benchRequestId || state.mode !== "stats") return;
      if (status) status.textContent = "Erreur métriques";
    });
}

function renderBenchMetrics(response) {
  const status = byId("bench-status");
  const visuals = byId("bench-visuals");
  const side = byId("bench-side-content");
  const details = byId("bench-details-content");

  if (!visuals) return;

  visuals.innerHTML = "";
  if (side) side.innerHTML = "";
  if (details) details.innerHTML = "";

  if (status) {
    const tone = response.status === "ok"
      ? (response.baseline_mode === "legacy_strict_fallback" ? "warning" : "ok")
      : "no-data";
    status.className = "bench-status " + tone;
    status.textContent = response.message || response.status;
  }

  if (response.status !== "ok" || !response.selected) {
    visuals.appendChild(noDataBlock(response));
    return;
  }

  visuals.appendChild(themeDashboardBlock(response));
}

function themeDashboardBlock(response) {
  const shell = document.createElement("div");
  shell.className = "theme-dashboard-shell";
  shell.appendChild(resultConfigStrip(response));

  const grid = document.createElement("div");
  grid.className = "theme-dashboard";
  for (const theme of THEME_DEFS) {
    if (theme.id === "result") continue;
    grid.appendChild(themeCard(theme, response));
  }
  shell.appendChild(grid);
  return shell;
}

function resultFoundLabel(value) {
  if (value == null) return "Résultat inconnu";
  return value === true || value === 1 || value === "true" || value === "1" ? "Chemin trouvé" : "Chemin non trouvé";
}

function resultConfigStrip(response) {
  const selected = response.selected;
  const validation = selected?.validation || {};
  const strip = document.createElement("button");
  strip.type = "button";
  strip.className = "result-config-strip";
  strip.innerHTML = `
      <span class="result-config-status">${escapeHtml(resultFoundLabel(validation.found))}</span>
      <span><strong>Coût</strong> ${escapeHtml(formatSpecialMetric("cost", validation.cost))}</span>
      <span><strong>Longueur</strong> ${escapeHtml(formatSpecialMetric("path_len", validation.path_len))}</span>
      <span class="result-config-source">Temps : perf · autres métriques : instrumenté</span>
      <span class="result-config-action">Résultat & configuration</span>
    `;
  strip.addEventListener("click", () => openThemeDetails("result"));
  return strip;
}

function themeCard(theme, response) {
  const selected = response.selected;
  const baseline = response.baseline;
  const delta = response.delta || {};
  const card = document.createElement("div");
  card.className = "theme-card";
  card.setAttribute("role", "button");
  card.setAttribute("tabindex", "0");
  card.dataset.theme = theme.id;

  const primaryValue = themePrimaryValue(theme, selected);
  const summaryRows = themeSummaryRows(theme.id, selected, baseline);
  const deltaText = themeDeltaText(theme.id, delta);

  card.innerHTML = `
      <div class="theme-card-head">
        <div class="theme-card-title">${escapeHtml(theme.title)}</div>
        <div class="theme-card-action">Détails</div>
      </div>
      ${theme.id === "memory"
      ? memoryCardBodyHtml(selected, baseline)
      : theme.id === "work"
        ? workCardBodyHtml(response)
        : theme.id === "locality"
          ? localityCardBodyHtml(selected, baseline)
          : `
              ${theme.id === "time"
            ? `<div class="time-card-primary-row">
                    <div class="theme-card-value">${escapeHtml(primaryValue)}</div>
                    ${tileIndexPrimaryHtml(response)}
                  </div>`
            : `<div class="theme-card-value">${escapeHtml(primaryValue)}</div>`}
              ${deltaText ? `<div class="delta-pill ${deltaText.kind}">${escapeHtml(deltaText.text)}</div>` : ""}
              ${theme.id === "time"
            ? `<div class="time-summary-groups compact">${timeSummaryGroupsHtml(summaryRows, "card")}</div>${tileIndexCompactComparisonHtml(response)}`
            : `<div class="theme-card-grid">${summaryRows.map((row) => miniStatHtml(row.label, row.value, row.baseline)).join("")}</div>`}
            `}
    `;

  card.addEventListener("click", () => openThemeDetails(theme.id));
  card.addEventListener("keydown", (event) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      openThemeDetails(theme.id);
    }
  });

  return card;
}

function lowerIsBetterDelta(selectedValue, baselineValue) {
  if (selectedValue == null || baselineValue == null) return null;
  const selected = Number(selectedValue);
  const baseline = Number(baselineValue);
  if (!Number.isFinite(selected) || !Number.isFinite(baseline)) return null;

  const delta = baseline - selected;
  if (Math.abs(delta) < 0.0005) return { label: "Écart", value: 0, kind: "neutral" };
  if (delta > 0) return { label: "Gain", value: delta, kind: "good" };
  return { label: "Perte", value: -delta, kind: "bad" };
}

function memoryPrimaryMetricHtml(label, value, metricKey) {
  return `
      <div class="memory-card-primary">
        <div class="memory-card-primary-label">${escapeHtml(label)}</div>
        <div class="theme-card-value">${escapeHtml(fmtMetric(metricKey, value))}</div>
      </div>
    `;
}

function memoryComparisonMetricHtml(label, selectedValue, baselineValue, metricKey) {
  const selected = Number(selectedValue);
  const baseline = Number(baselineValue);
  const hasSelected = selectedValue != null && Number.isFinite(selected);
  const hasBaseline = baselineValue != null && Number.isFinite(baseline);
  const maximum = Math.max(hasSelected ? selected : 0, hasBaseline ? baseline : 0, 1);
  const selectedWidth = hasSelected ? clamp((selected / maximum) * 100, 0, 100) : 0;
  const baselineWidth = hasBaseline ? clamp((baseline / maximum) * 100, 0, 100) : 0;

  return `
      <div class="memory-compare-metric">
        <div class="memory-compare-head">
          <strong>${escapeHtml(label)}</strong>
        </div>
        <div class="memory-compare-line">
          <span>Sélection</span>
          <div class="memory-compare-track"><div class="memory-compare-fill neutral" style="width:${selectedWidth}%"></div></div>
          <strong>${escapeHtml(fmtMetric(metricKey, selectedValue))}</strong>
        </div>
        <div class="memory-compare-line">
          <span>${escapeHtml(baselineAlgoLabel(true))}</span>
          <div class="memory-compare-track"><div class="memory-compare-fill neutral" style="width:${baselineWidth}%"></div></div>
          <strong>${escapeHtml(fmtMetric(metricKey, baselineValue))}</strong>
        </div>
      </div>
    `;
}

function memoryCardComparisonGroupHtml(label, selectedValue, baselineValue, metricKey) {
  const delta = lowerIsBetterDelta(selectedValue, baselineValue);
  return `
      <div class="memory-card-comparison-group">
        <div class="theme-card-grid">
          ${miniStatHtml(label, fmtMetric(metricKey, baselineValue), null)}
          ${miniStatHtml(
    delta ? delta.label : "Écart",
    delta ? fmtMetric(metricKey, delta.value) : "—",
    null,
    delta ? delta.kind : "",
  )}
        </div>
      </div>
    `;
}

function memoryCardBodyHtml(selected, baseline) {
  const reads = metricOf(selected, "search_flash_reads");
  const writes = metricOf(selected, "search_flash_writes");
  const baseReads = metricOf(baseline, "search_flash_reads");
  const baseWrites = metricOf(baseline, "search_flash_writes");
  const collectionPeak = metricOf(selected, "max_collection_bytes");
  const baseCollectionPeak = metricOf(baseline, "max_collection_bytes");
  const accesses = metricOf(selected, "search_cache_accesses");
  const missesPerMillion = metricOf(selected, "search_cache_misses_per_million");

  return `
      <div class="memory-card-primary-grid">
        ${memoryPrimaryMetricHtml("Lectures flash", reads, "search_flash_reads")}
        ${memoryPrimaryMetricHtml("Écritures flash", writes, "search_flash_writes")}
      </div>
      <div class="memory-card-comparison">
        ${comparisonFieldsetHtml(
    baselineComparisonTitle(),
    `
            ${memoryCardComparisonGroupHtml("Lectures flash", reads, baseReads, "search_flash_reads")}
            ${memoryCardComparisonGroupHtml("Écritures flash", writes, baseWrites, "search_flash_writes")}
            ${memoryCardComparisonGroupHtml("Plus grosse collection au peak", collectionPeak, baseCollectionPeak, "max_collection_bytes")}
          `,
  )}
      </div>
      <div class="memory-card-context">
        <div class="time-summary-group-title">Contexte cache</div>
        <div class="theme-card-grid memory-card-context-grid">
          ${miniStatHtml("accès cache", fmtMetric("search_cache_accesses", accesses), null)}
          ${miniStatHtml("défauts / 1M", missesPerMillion == null ? "—" : fmt(missesPerMillion, missesPerMillion >= 100 ? 0 : 1), null)}
        </div>
      </div>
    `;
}

function localityCardBodyHtml(selected, baseline) {
  const visitedCross = metricOf(selected, "visited_cross_tile_rate");
  const avgVisitedJump = metricOf(selected, "avg_visited_tile_jump");
  const baseVisitedCross = metricOf(baseline, "visited_cross_tile_rate");
  const baseAvgVisitedJump = metricOf(baseline, "avg_visited_tile_jump");
  const neighborsNextCross = metricOf(selected, "neighbors_next_cross_tile_rate");
  const borderEdges = metricOf(selected, "edges_from_border_node_rate");

  return `
      <div class="locality-card-primary-grid">
        ${memoryPrimaryMetricHtml("Visited cross", visitedCross, "visited_cross_tile_rate")}
        ${memoryPrimaryMetricHtml("Avg visited jump", avgVisitedJump, "avg_visited_tile_jump")}
      </div>
      <div class="locality-card-comparison">
        ${comparisonFieldsetHtml(
    baselineComparisonTitle(),
    `
            ${workCardComparisonGroupHtml("Visited cross", "visited_cross_tile_rate", visitedCross, baseVisitedCross, "lower")}
            ${workCardComparisonGroupHtml("Avg visited jump", "avg_visited_tile_jump", avgVisitedJump, baseAvgVisitedJump, "lower")}
          `,
  )}
      </div>
      <div class="locality-card-context">
        <div class="time-summary-group-title">Contexte du graphe</div>
        <div class="theme-card-grid locality-card-context-grid">
          ${miniStatHtml("neighbors next cross", fmtMetric("neighbors_next_cross_tile_rate", neighborsNextCross), null)}
          ${miniStatHtml("edges from border nodes", fmtMetric("edges_from_border_node_rate", borderEdges), null)}
        </div>
      </div>
    `;
}

function signedMetricDeltaHtmlValue(metricKey, selectedValue, baselineValue) {
  if (selectedValue == null || selectedValue === "" || baselineValue == null || baselineValue === "") return "—";
  const selected = Number(selectedValue);
  const baseline = Number(baselineValue);
  if (!Number.isFinite(selected) || !Number.isFinite(baseline)) return "—";
  const delta = selected - baseline;
  const sign = delta > 0 ? "+" : delta < 0 ? "−" : "";
  const magnitude = Math.abs(delta);
  if (metricKey.endsWith("_bytes")) {
    return sign + fmtBytes(magnitude);
  }
  if (metricKey.endsWith("_rate") || metricKey.endsWith("_ratio")) {
    return sign + fmt(magnitude * 100, 1) + " %";
  }
  if (metricKey.includes("per_")) {
    return sign + fmt(magnitude, 2);
  }
  return sign + fmt(magnitude, magnitude >= 100 ? 0 : magnitude >= 10 ? 1 : 2);
}

function workDeltaKind(selectedValue, baselineValue, better) {
  if (selectedValue == null || selectedValue === "" || baselineValue == null || baselineValue === "") return "";
  const selected = Number(selectedValue);
  const baseline = Number(baselineValue);
  if (!Number.isFinite(selected) || !Number.isFinite(baseline) || selected === baseline) return "";
  const improved = better === "higher" ? selected > baseline : selected < baseline;
  return improved ? "good" : "bad";
}

function workComparisonSummary(selectedValue, baselineValue, better) {
  if (selectedValue == null || selectedValue === "" || baselineValue == null || baselineValue === "") {
    return { label: "Écart", kind: "", delta: null };
  }
  const selected = Number(selectedValue);
  const baseline = Number(baselineValue);
  if (!Number.isFinite(selected) || !Number.isFinite(baseline)) {
    return { label: "Écart", kind: "", delta: null };
  }
  if (selected === baseline) {
    return { label: "Écart", kind: "", delta: 0 };
  }
  const improved = better === "higher" ? selected > baseline : selected < baseline;
  return {
    label: improved ? "Gain" : "Perte",
    kind: improved ? "good" : "bad",
    delta: Math.abs(selected - baseline),
  };
}

function workCardComparisonGridHtml(label, metricKey, selectedValue, baselineValue, better) {
  const comparison = workComparisonSummary(selectedValue, baselineValue, better);
  const comparisonValue = comparison.delta == null ? "—" : fmtMetric(metricKey, comparison.delta);
  return `
      <div class="theme-card-grid work-card-compare-grid">
        ${miniStatHtml(label, fmtMetric(metricKey, baselineValue), null)}
        ${miniStatHtml(comparison.label, comparisonValue, null, comparison.kind)}
      </div>
    `;
}

function workCardPrimaryHtml(label, metricKey, selectedValue) {
  return `
      <div class="work-card-primary">
        <div class="memory-card-primary-label">${escapeHtml(label)}</div>
        <div class="theme-card-value">${escapeHtml(fmtMetric(metricKey, selectedValue))}</div>
      </div>
    `;
}

function workCardComparisonGroupHtml(label, metricKey, selectedValue, baselineValue, better) {
  return `
      <div class="memory-card-comparison-group">
        ${workCardComparisonGridHtml(label, metricKey, selectedValue, baselineValue, better)}
      </div>
    `;
}

function comparisonFieldsetHtml(title, bodyHtml, leftHeader = "Valeurs de référence", rightHeader = "Gain / perte sur ce run") {
  return `
      <fieldset class="comparison-fieldset">
        <legend>${escapeHtml(title)}</legend>
        <div class="comparison-fieldset-head">
          <span>${escapeHtml(leftHeader)}</span>
          <span>${escapeHtml(rightHeader)}</span>
        </div>
        <div class="comparison-fieldset-body">
          ${bodyHtml}
        </div>
      </fieldset>
    `;
}

function workCardSectionHtml(title, statsHtml) {
  return `
      <div class="work-card-section">
        <div class="time-summary-group-title">${escapeHtml(title)}</div>
        <div class="theme-card-grid work-card-compare-grid">
          ${statsHtml}
        </div>
      </div>
    `;
}

function workCardRelativeComparisonGroupHtml(referenceLabel, selectedValue, baselineValue, better, metricKey = "graph_edge_work", labels = null) {
  const comparison = workComparisonSummary(selectedValue, baselineValue, better);
  const relative = relativeDeltaRatio(selectedValue, baselineValue);
  const comparisonValue = relative == null ? "—" : `${fmt(Math.abs(relative) * 100, 1)} %`;
  const comparisonLabel = comparison.delta === 0
    ? "Écart"
    : comparison.kind === "bad"
      ? labels?.bad || comparison.label
      : labels?.good || comparison.label;
  return `
      <div class="memory-card-comparison-group">
        <div class="theme-card-grid work-card-compare-grid">
          ${miniStatHtml(referenceLabel, fmtMetric(metricKey, baselineValue), null)}
          ${miniStatHtml(comparisonLabel, comparisonValue, null, comparison.kind)}
        </div>
      </div>
    `;
}

function workComponentTone(selectedValue, baselineValue) {
  const relative = relativeDeltaRatio(selectedValue, baselineValue);
  if (relative == null || relative === 0) return { kind: "", strength: 0 };
  const abs = Math.abs(relative);
  let strength;
  if (abs < 0.001) {
    strength = 0.03 + 0.12 * (abs / 0.001);
  } else if (abs < 0.01) {
    strength = 0.15 + 0.25 * ((abs - 0.001) / 0.009);
  } else {
    strength = 0.40 + 0.50 * Math.min((abs - 0.01) / 0.49, 1);
  }
  return { kind: relative > 0 ? "bad" : "good", strength };
}

function relativePercentText(value) {
  if (value == null || !Number.isFinite(Number(value))) return "—";
  const n = Number(value);
  const sign = n > 0 ? "+" : n < 0 ? "−" : "";
  const absPercent = Math.abs(n) * 100;
  const digits = absPercent < 0.1 ? 3 : absPercent < 1 ? 2 : 1;
  return `${sign}${fmt(absPercent, digits)} %`;
}

function blendRgbColors(startHex, endHex, t) {
  const clamp01 = clamp(t, 0, 1);
  const parseHex = (hex) => {
    const value = hex.replace("#", "");
    return [0, 2, 4].map((offset) => parseInt(value.slice(offset, offset + 2), 16));
  };
  const [sr, sg, sb] = parseHex(startHex);
  const [er, eg, eb] = parseHex(endHex);
  const mix = (a, b) => Math.round(a + (b - a) * clamp01);
  return `rgb(${mix(sr, er)}, ${mix(sg, eg)}, ${mix(sb, eb)})`;
}

function workComponentValueColor(kind, strength) {
  if (!kind) return null;
  const base = "#e8eef8";
  const target = kind === "good" ? "#86efac" : "#fda4af";
  return blendRgbColors(base, target, strength);
}

function workComponentStatHtml(label, metricKey, selectedValue, baselineValue) {
  const tone = workComponentTone(selectedValue, baselineValue);
  const relative = relativeDeltaRatio(selectedValue, baselineValue);
  const valueStyle = tone.kind ? ` style="color:${workComponentValueColor(tone.kind, tone.strength)}"` : "";
  const baselineText = baselineValue == null || baselineValue === ""
    ? "réf. : —"
    : `réf. : ${fmtMetric(metricKey, baselineValue)} · ${relativePercentText(relative)}`;
  return `
      <div class="theme-mini-stat work-component-stat">
        <div class="theme-mini-label">${escapeHtml(label)}</div>
        <div class="theme-mini-value"${valueStyle}>${escapeHtml(fmtMetric(metricKey, selectedValue))}</div>
        <div class="metric-baseline">${escapeHtml(baselineText)}</div>
      </div>
    `;
}

function relativeDeltaRatio(selectedValue, baselineValue) {
  if (selectedValue == null || selectedValue === "" || baselineValue == null || baselineValue === "") return null;
  const selected = Number(selectedValue);
  const baseline = Number(baselineValue);
  if (!Number.isFinite(selected) || !Number.isFinite(baseline) || baseline === 0) return null;
  return (selected - baseline) / baseline;
}

function astarWorkBaseline(response = state.benchLastResponse) {
  if (!response) return null;
  return response.work_algorithms?.astar_lazy
    || (normalizeBenchAlgoId(response.baseline_query?.algo) === "astar_lazy" ? response.baseline : null)
    || (normalizeBenchAlgoId(response.query?.algo) === "astar_lazy" ? response.selected : null);
}

function workCardBodyHtml(response) {
  const selected = response?.selected;
  const baseline = response?.baseline || astarWorkBaseline(response);
  const work = metricOf(selected, "algorithmic_work");
  const baseWork = metricOf(baseline, "algorithmic_work");
  const graphWork = metricOf(selected, "algorithmic_work_graph");
  const frontierWork = metricOf(selected, "algorithmic_work_frontier");
  const stateWork = metricOf(selected, "algorithmic_work_state");
  const heuristicWork = metricOf(selected, "algorithmic_work_heuristic");
  const baseGraphWork = metricOf(baseline, "algorithmic_work_graph");
  const baseFrontierWork = metricOf(baseline, "algorithmic_work_frontier");
  const baseStateWork = metricOf(baseline, "algorithmic_work_state");
  const baseHeuristicWork = metricOf(baseline, "algorithmic_work_heuristic");
  const visitedPerPath = metricOf(selected, "visited_per_path_node");
  const pruned = metricOf(selected, "neighbors_skipped_ratio");

  return `
      <div class="work-card-primary-grid">
        ${workCardPrimaryHtml("Appels instrumentés", "algorithmic_work", work)}
      </div>
      <div class="work-card-comparison">
        <div class="time-summary-group-title">${escapeHtml(baselineComparisonTitle(response))}</div>
        ${workCardRelativeComparisonGroupHtml(
    "Appels de référence",
    work,
    baseWork,
    "lower",
    "algorithmic_work",
    { bad: "Surcroît d’appels", good: "Réduction d’appels" },
  )}
        ${workCardSectionHtml("Décomposition des appels", `
          ${workComponentStatHtml("Graphe · neighbors open/next", "algorithmic_work_graph", graphWork, baseGraphWork)}
          ${workComponentStatHtml("Frontière · heap/global/local", "algorithmic_work_frontier", frontierWork, baseFrontierWork)}
          ${workComponentStatHtml("État · done/previous/states", "algorithmic_work_state", stateWork, baseStateWork)}
          ${workComponentStatHtml("Heuristique · calls", "algorithmic_work_heuristic", heuristicWork, baseHeuristicWork)}
        `)}
        ${workCardSectionHtml("Indicateurs secondaires", `
          ${miniStatHtml("Visited nodes / path len", fmtMetric("visited_per_path_node", visitedPerPath), null)}
          ${miniStatHtml("Neighbors skipped", fmtMetric("neighbors_skipped_ratio", pruned), null)}
        `)}
      </div>
    `;
}

function themePrimaryValue(theme, aggregate) {
  if (!aggregate) return "—";

  if (theme.id === "result") {
    const found = aggregate.validation?.found;
    if (found != null) return found === "true" || found === "1" ? "found" : String(found);
  }

  const value = theme.id === "time"
    ? metricOf(aggregate, "search_ms")
    : metricOf(aggregate, theme.primaryMetric);

  return fmtMetric(theme.primaryMetric, value);
}

function themeDeltaText() {
  return null;
}

function algorithmThemeSummaryMetrics(themeId) {
  return THEME_SUMMARY_METRICS[themeId] || [];
}

function themeSummaryRows(themeId, selected, baseline) {
  if (themeId === "time") return timeThemeSummaryRows(selected, baseline);

  const rows = algorithmThemeSummaryMetrics(themeId);

  return rows.map(([key, label]) => {
    const value = specialMetricValue(selected, key);
    const base = specialMetricValue(baseline, key);

    return {
      key,
      label,
      value: formatSpecialMetric(key, value),
      baseline: base == null ? null : formatSpecialMetric(key, base),
    };
  });
}

function hostDurationMs(aggregate) {
  const searchMs = metricOf(aggregate, "search_ms");
  const projectedIoMs = metricOf(aggregate, "search_flash_estimated_ms");
  if (searchMs == null || projectedIoMs == null) return null;
  return searchMs - projectedIoMs;
}

function rp2040CpuSlowdownFactor() {
  return HOST_REFERENCE_CPU_MHZ / RP2040_CPU_MHZ;
}

function rp2040HostEstimatedMs(aggregate) {
  const hostMs = hostDurationMs(aggregate);
  if (hostMs == null) return null;
  return hostMs * rp2040CpuSlowdownFactor();
}

function rp2040TotalEstimatedMs(aggregate) {
  const projectedIoMs = metricOf(aggregate, "search_flash_estimated_ms");
  const hostEstimatedMs = rp2040HostEstimatedMs(aggregate);
  if (projectedIoMs == null || hostEstimatedMs == null) return null;
  return projectedIoMs + hostEstimatedMs;
}

function timeThemeSummaryRows(selected, baseline) {
  const searchMs = metricOf(selected, "search_ms");
  const baseSearchMs = metricOf(baseline, "search_ms");
  const projectedIoMs = metricOf(selected, "search_flash_estimated_ms");
  const hostMs = hostDurationMs(selected);
  const hostRp2040Ms = rp2040HostEstimatedMs(selected);
  const totalRp2040Ms = rp2040TotalEstimatedMs(selected);
  const gainMs = searchMs == null || baseSearchMs == null ? null : baseSearchMs - searchMs;

  return [
    {
      group: baselineComparisonTitle(),
      key: "baseline_search_ms",
      label: "Durée de référence",
      value: fmtMetric("search_ms", baseSearchMs),
      baseline: null,
    },
    {
      group: baselineComparisonTitle(),
      key: "search_gain_ms",
      label: gainMs != null && gainMs < 0 ? "Perte" : "Gain",
      value: gainMs == null ? "—" : fmtMetric("search_ms", Math.abs(gainMs)),
      baseline: null,
      kind: gainMs == null || gainMs === 0 ? "" : (gainMs > 0 ? "good" : "bad"),
    },
    {
      group: "Décomposition du run",
      key: "host_duration_ms",
      label: "Durée Hôte",
      value: fmtMetric("search_ms", hostMs),
      baseline: null,
    },
    {
      group: "Décomposition du run",
      key: "search_flash_estimated_ms",
      label: "Durée E/S projetée",
      value: fmtMetric("search_ms", projectedIoMs),
      baseline: null,
    },
    {
      group: "Extrapolation RP2040",
      key: "rp2040_host_estimated_ms",
      label: "Durée Hôte estimée",
      value: fmtMetric("search_ms", hostRp2040Ms),
      baseline: null,
    },
    {
      group: "Extrapolation RP2040",
      key: "rp2040_total_estimated_ms",
      label: "Durée totale estimée",
      value: fmtMetric("search_ms", totalRp2040Ms),
      baseline: null,
    },
  ];
}

function timeCoverageNoteHtml(response) {
  const note = response?.baseline_mode === "legacy_strict_fallback"
    ? `La comparaison affichée utilise <strong>${escapeHtml(baselineAlgoLabel(false, response))}</strong> en <strong>équivalent strict legacy</strong> : toutes les options compatibles avec la référence sont conservées ; seules les options qu’elle ne supporte pas reviennent à leur équivalent legacy.`
    : `La référence est <strong>${escapeHtml(baselineAlgoLabel(false, response))}</strong> avec les mêmes scénario, profil RAM, layout, abstraction du graphe et abstraction des voisins (comparaison TCEPA). Les options propres à l’algorithme sont reprises lorsqu’elles s’appliquent : structuration des données, politique d’exploration et heuristique lorsqu’elles sont configurables.`;
  return `${note} Aucun réglage séparé de la référence n’est nécessaire. Une référence sans données pour cette configuration est indiquée comme indisponible. La <strong>durée de recherche</strong> vient de l’exécution perf. La <strong>durée E/S projetée</strong> est calculée depuis l’exécution instrumentée : lectures×${FLASH_READ_LATENCY_MS} ms + écritures×${FLASH_WRITE_LATENCY_MS} ms. La <strong>durée Hôte</strong> vaut search_ms − durée E/S projetée et dépend de la machine de benchmark. L'extrapolation RP2040 applique uniquement le ratio de fréquences ${HOST_REFERENCE_CPU_MHZ}/${RP2040_CPU_MHZ} au temps hôte ; c'est un indicateur de sensibilité CPU, pas une mesure sur RP2040.`;
}

function tileIndexImpactData(response) {
  const comparison = response?.graph_storage_comparison || {};
  const dichoSearchMs = metricOf(comparison.tile_dicho, "search_ms");
  const indexedSearchMs = metricOf(comparison.tile_index, "search_ms");
  const dichoGraphMs = metricOf(comparison.tile_dicho, "graph_ms");
  const indexedGraphMs = metricOf(comparison.tile_index, "graph_ms");

  const indexMs = dichoGraphMs == null || indexedGraphMs == null
    ? null
    : indexedGraphMs - dichoGraphMs;
  const validIndexMs = indexMs != null && Number.isFinite(indexMs) && indexMs >= 0 ? indexMs : null;
  const firstIndexedMs = validIndexMs == null || indexedSearchMs == null
    ? null
    : validIndexMs + indexedSearchMs;
  const netGainMs = dichoSearchMs == null || firstIndexedMs == null
    ? null
    : dichoSearchMs - firstIndexedMs;
  const readyGainMs = dichoSearchMs == null || indexedSearchMs == null
    ? null
    : dichoSearchMs - indexedSearchMs;
  const readyGainRate = readyGainMs == null || !(dichoSearchMs > 0)
    ? null
    : readyGainMs / dichoSearchMs;
  const breakEvenQueries = validIndexMs == null || !(readyGainMs > 0)
    ? null
    : Math.max(1, Math.ceil(validIndexMs / readyGainMs));

  return {
    dichoSearchMs,
    indexedSearchMs,
    indexMs: validIndexMs,
    firstIndexedMs,
    netGainMs,
    readyGainMs,
    readyGainRate,
    breakEvenQueries,
  };
}

function tileIndexPrimaryHtml(response) {
  if (response?.query?.graph_storage !== "tile_index") return "";
  const impact = tileIndexImpactData(response);
  if (impact.indexMs == null) return "";
  return `
      <div class="time-index-overhead" title="graph_ms(tile_index) − graph_ms(tile_dicho)">
        <div class="time-index-overhead-value">(+${escapeHtml(fmtMetric("search_ms", impact.indexMs))})</div>
        <div class="time-index-overhead-label">indexage</div>
      </div>
    `;
}

function tileIndexCompactComparisonHtml(response) {
  if (response?.query?.graph_storage !== "tile_index") return "";
  const impact = tileIndexImpactData(response);
  if (impact.dichoSearchMs == null || impact.netGainMs == null) return "";
  const delta = impact.netGainMs;
  const neutral = Math.abs(delta) < 0.0005;
  const label = neutral ? "Écart" : (delta > 0 ? "Gain" : "Perte");
  const kind = neutral ? "" : (delta > 0 ? "good" : "bad");
  return `
      <div class="time-index-comparison">
        <div class="time-summary-group-title">Comparaison abstraction de graphe minimale</div>
        <div class="time-index-comparison-grid">
          ${miniStatHtml("Durée sans indexage", fmtMetric("search_ms", impact.dichoSearchMs), null)}
          ${miniStatHtml(label, fmtMetric("search_ms", Math.abs(delta)), null, kind)}
        </div>
      </div>
    `;
}

function tileIndexImpactHtml(response) {
  if (response?.query?.graph_storage !== "tile_index") return "";

  const impact = tileIndexImpactData(response);
  const { dichoSearchMs, indexedSearchMs, indexMs, readyGainMs, readyGainRate, breakEvenQueries } = impact;
  const duration = (value) => value == null ? "—" : formatReplayDuration(value);

  if (dichoSearchMs == null || indexedSearchMs == null) {
    return `
        <h3>Impact de l’index de tuiles</h3>
        <div class="modal-empty">Comparaison Minimale / Tuiles indexées indisponible.</div>
      `;
  }

  const neutral = readyGainMs == null || Math.abs(readyGainMs) < 0.0005;
  const gainKind = neutral ? "" : (readyGainMs > 0 ? "good" : "bad");
  const gainLabel = neutral ? "Écart / requête, index prêt" : (readyGainMs > 0 ? "Gain / requête, index prêt" : "Surcoût / requête, index prêt");
  const rateLabel = neutral ? "Écart relatif" : (readyGainMs > 0 ? "Accélération" : "Ralentissement");
  const payback = readyGainMs > 0
    ? (indexMs === 0 ? "Immédiat" : breakEvenQueries === 1 ? "Dès la 1re requête" : `À partir de ${breakEvenQueries} requêtes`)
    : "Non amorti";
  const visualMaximum = Math.max(indexMs || 0, Math.abs(readyGainMs || 0), 1);
  const indexWidth = indexMs == null ? 0 : clamp((indexMs / visualMaximum) * 100, 0, 100);
  const gainWidth = readyGainMs == null ? 0 : clamp((Math.abs(readyGainMs) / visualMaximum) * 100, 0, 100);
  const indexEntries = metricOf(response.selected, "graph_index_entries");
  const indexBytes = metricOf(response.selected, "graph_index_logical_bytes");
  const fallbackTiles = metricOf(response.selected, "graph_index_fallback_tiles");
  const hasIndexContext = indexEntries != null || indexBytes != null || fallbackTiles != null;

  return `
      <div class="index-impact">
        <h3>Impact de l’index de tuiles</h3>
        <div class="index-impact-kpis">
          ${modalKpiHtml(gainLabel, duration(Math.abs(readyGainMs || 0)), gainKind)}
          ${modalKpiHtml(rateLabel, readyGainRate == null ? "—" : `${fmt(Math.abs(readyGainRate) * 100, 1)} %`, gainKind)}
          ${modalKpiHtml("Amortissement", payback, readyGainMs > 0 ? "good" : (neutral ? "" : "bad"))}
        </div>
        <div class="index-impact-detail-grid">
          <div class="index-payback-visual" aria-label="Coût d’indexage comparé au gain par requête">
            <div class="index-payback-row">
              <span>Indexage · coût unique</span>
              <div class="theme-bar-track"><div class="theme-bar-fill preparation" style="width:${indexWidth}%"></div></div>
              <strong>${escapeHtml(duration(indexMs))}</strong>
            </div>
            <div class="index-payback-row">
              <span>${readyGainMs < 0 ? "Surcoût" : "Gain"} · par requête</span>
              <div class="theme-bar-track"><div class="theme-bar-fill ${readyGainMs < 0 ? "bad" : "good"}" style="width:${gainWidth}%"></div></div>
              <strong>${escapeHtml(duration(Math.abs(readyGainMs || 0)))}</strong>
            </div>
          </div>
          ${hasIndexContext ? `
            <div class="index-impact-context">
              <div><span>Entrées indexées</span><strong>${escapeHtml(indexEntries == null ? "—" : fmt(indexEntries, 0))}</strong></div>
              <div><span>Taille logique</span><strong>${escapeHtml(indexBytes == null ? "—" : fmtBytes(indexBytes))}</strong></div>
              <div><span>Tuiles en fallback</span><strong>${escapeHtml(fallbackTiles == null ? "—" : fmt(fallbackTiles, 0))}</strong></div>
            </div>
          ` : ""}
        </div>
        <p class="metric-coverage-note">Le coût d’indexage est payé une fois ; le gain par requête compare ensuite « Tuiles indexées » à « Minimale », index prêt.</p>
      </div>
    `;
}

function timeSummaryGroupsHtml(rows, variant) {
  const groups = [];
  for (const row of rows) {
    let group = groups.find((entry) => entry.title === row.group);
    if (!group) {
      group = { title: row.group || "", rows: [] };
      groups.push(group);
    }
    group.rows.push(row);
  }

  return groups.map((group) => `
      <div class="time-summary-group">
        <div class="time-summary-group-title">${escapeHtml(group.title)}</div>
        <div class="${variant === "modal" ? "modal-kpi-grid" : "theme-card-grid"}">
          ${group.rows.map((row) => variant === "modal"
    ? modalKpiHtml(row.label, row.value, row.kind)
    : miniStatHtml(row.label, row.value, row.baseline, row.kind)).join("")}
        </div>
      </div>
    `).join("");
}

function miniStatHtml(label, value, baseline, kind = "") {
  return `
      <div class="theme-mini-stat ${kind ? `metric-tone-${kind}` : ""}">
        <div class="theme-mini-label">${escapeHtml(label)}</div>
        <div class="theme-mini-value">${escapeHtml(value)}</div>
        ${baseline != null ? `<div class="metric-baseline">réf. : ${escapeHtml(baseline)}</div>` : ""}
      </div>
    `;
}

function specialMetricValue(aggregate, key) {
  if (!aggregate) return null;
  if (key === "found") return aggregate.validation?.found;
  if (key === "cost") return aggregate.validation?.cost ?? metricOf(aggregate, key);
  if (key === "path_len") return aggregate.validation?.path_len ?? metricOf(aggregate, key);
  return metricOf(aggregate, key);
}

function formatSpecialMetric(key, value) {
  if (value == null || value === "") return "—";
  const n = Number(value);
  if (Number.isFinite(n)) return fmtMetric(key, n);
  return String(value);
}

export {
  astarWorkBaseline,
  fetchBenchResult,
  formatSpecialMetric,
  memoryComparisonMetricHtml,
  resultFoundLabel,
  signedMetricDeltaHtmlValue,
  themeSummaryRows,
  tileIndexImpactHtml,
  timeCoverageNoteHtml,
  timeSummaryGroupsHtml,
  workDeltaKind,
};
