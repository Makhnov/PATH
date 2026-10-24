// Détails des statistiques et commandes de benchmark.

import { byId, clamp, escapeHtml, fmt, fmtBytes, fmtMetric, numberOrZero, setStatus } from "../utils.js";
import {
  FLASH_READ_LATENCY_MS,
  FLASH_WRITE_LATENCY_MS,
  PHASE_PREFIXES,
  THEME_DEFS,
  state,
} from "../state.js";
import {
  benchAlgorithmDescriptor,
  benchOptionLabel,
  bucketVariantKey,
  hasStateOptions,
  isBucketAlgo,
  isBucketDecoupled,
  isCountdownAlgo,
  normalizeBenchAlgoId,
  usesDecoupledState,
} from "../controls.js";
import {
  astarWorkBaseline,
  formatSpecialMetric,
  memoryComparisonMetricHtml,
  resultFoundLabel,
  signedMetricDeltaHtmlValue,
  themeSummaryRows,
  tileIndexImpactHtml,
  timeCoverageNoteHtml,
  timeSummaryGroupsHtml,
  workDeltaKind,
} from "./dashboard.js";
import { metricDisplayKey, metricOf, metricSourceLabel, phaseLabel, themeMetricKeys } from "./metrics.js";

function installBenchModalHandlers() {
  const modal = byId("bench-modal");
  if (!modal || modal.dataset.installed === "1") return;

  modal.dataset.installed = "1";
  modal.addEventListener("click", (event) => {
    if (event.target.closest("[data-bench-modal-close]")) closeBenchModal();
  });

  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && !modal.hidden) closeBenchModal();
  });
}

function closeBenchModal() {
  const modal = byId("bench-modal");
  if (!modal) return;

  modal.hidden = true;
  modal.setAttribute("aria-hidden", "true");
}

function openThemeDetails(themeId) {
  const response = state.benchLastResponse;
  const theme = THEME_DEFS.find((item) => item.id === themeId);
  const modal = byId("bench-modal");
  const title = byId("bench-modal-title");
  const content = byId("bench-modal-content");

  if (!response?.selected || !theme || !modal || !title || !content) return;

  const kicker = byId("bench-modal-kicker");
  if (kicker) kicker.textContent = modalKicker(response);
  title.textContent = theme.title;
  modal.dataset.theme = theme.id;
  content.replaceChildren(themeModalContent(theme, response));
  modal.hidden = false;
  modal.setAttribute("aria-hidden", "false");
}

function modalKicker(response) {
  const q = response.query || {};
  const parts = [q.case_id, benchAlgorithmDescriptor(q.algo)?.label || q.algo, q.gps_layout, q.swap_profile].filter(Boolean);
  parts.push(`réf. : ${baselineAlgoLabel(true, response)} (${baselineComparisonTag(response)})`);
  return parts.join(" · ");
}

function themeModalContent(theme, response) {
  const selected = response.selected;
  const baseline = response.baseline;

  if (theme.id === "result") {
    const root = document.createElement("div");
    root.className = "modal-layout result-modal-layout";
    root.appendChild(resultContextSection(response));
    root.appendChild(resultRunCommandsSection(response));
    return root;
  }

  if (theme.id === "time") {
    const root = document.createElement("div");
    root.className = "modal-layout time-modal-layout";
    root.appendChild(modalOverviewSection(theme, response));
    root.appendChild(timePhaseSection(selected));

    if (response.query?.graph_storage === "tile_index") {
      const indexImpact = document.createElement("section");
      indexImpact.className = "modal-section modal-span-all";
      indexImpact.innerHTML = tileIndexImpactHtml(response);
      root.appendChild(indexImpact);
    }

    return root;
  }

  if (theme.id === "memory") {
    const root = document.createElement("div");
    root.className = "modal-layout memory-modal-layout";
    root.appendChild(memoryComparisonSection(selected, baseline));
    root.appendChild(flashReadDistributionSection(selected));
    root.appendChild(flashWriteDistributionSection(selected));
    root.appendChild(cacheMissRateByOperationSection(selected));
    root.appendChild(flashWritesPerKByOperationSection(selected));
    root.appendChild(workCollectionsSection(selected, baseline));
    const matrix = cacheOperationMatrixSection(selected);
    matrix.classList.add("modal-span-all");
    root.appendChild(matrix);
    return root;
  }

  if (theme.id === "work") {
    const root = document.createElement("div");
    const workBaseline = response.baseline || astarWorkBaseline(response);
    root.className = "modal-layout work-modal-layout";
    const raw = workRawComparisonSection(selected, workBaseline, response);
    raw.classList.add("modal-span-all");
    root.appendChild(raw);
    root.appendChild(workPruningSection(selected, workBaseline));
    const common = workCommonMetricsSection(response, workBaseline);
    common.classList.add("modal-span-all");
    root.appendChild(common);
    const specific = workSpecificMetricsSection(selected);
    specific.classList.add("modal-span-all");
    root.appendChild(specific);
    return root;
  }

  if (theme.id === "locality") {
    const root = document.createElement("div");
    root.className = "modal-layout locality-modal-layout";
    root.appendChild(localityCrossComparisonSection(selected, baseline));
    root.appendChild(localityJumpComparisonSection(selected, baseline));
    const context = localityGraphContextSection(selected, baseline);
    context.classList.add("modal-span-all");
    root.appendChild(context);
    const algorithms = localityAlgorithmsTableSection(response);
    algorithms.classList.add("modal-span-all");
    root.appendChild(algorithms);
    return root;
  }

  const root = document.createElement("div");
  root.className = "modal-layout";
  const left = document.createElement("div");
  left.className = "modal-column";
  const right = document.createElement("div");
  right.className = "modal-column";

  left.appendChild(modalOverviewSection(theme, response));

  right.appendChild(themeMetricsTableSection(theme, selected, baseline));
  root.appendChild(left);
  root.appendChild(right);
  return root;
}

function modalOverviewSection(theme, response) {
  const selected = response.selected;
  const section = document.createElement("section");
  section.className = "modal-section";
  const rows = themeSummaryRows(theme.id, selected, response.baseline);

  section.innerHTML = `
      <h3>Synthèse</h3>
      ${theme.id === "time"
      ? `<div class="time-summary-groups">${timeSummaryGroupsHtml(rows, "modal")}</div>`
      : `<div class="modal-kpi-grid">${rows.map((row) => modalKpiHtml(row.label, row.value)).join("")}</div>`}
      ${theme.id === "time" ? `<p class="metric-coverage-note">${timeCoverageNoteHtml(response)}</p>` : ""}
    `;

  return section;
}

function modalKpiHtml(label, value, kind = "") {
  return `
      <div class="modal-kpi ${kind ? `metric-tone-${kind}` : ""}">
        <div class="modal-kpi-label">${escapeHtml(label)}</div>
        <div class="modal-kpi-value">${escapeHtml(value)}</div>
      </div>
    `;
}

function resultContextSection(response) {
  const selected = response.selected;
  const baseline = response.baseline;
  const scenario = response.scenario;
  const q = response.query || {};
  const validation = selected.validation || {};
  const stable = validation.found_stable && validation.cost_stable && validation.path_len_stable;
  const sameAsBaseline = baseline
    && validation.found === baseline.validation?.found
    && validation.cost === baseline.validation?.cost
    && validation.path_len === baseline.validation?.path_len;

  const section = document.createElement("section");
  section.className = "modal-section modal-span-all result-context-section";
  section.innerHTML = `
      <div class="result-detail-head">
        <span class="result-status ${validation.found === "true" || validation.found === "1" ? "good" : "bad"}">${escapeHtml(resultFoundLabel(validation.found))}</span>
        <span><strong>Coût</strong> ${escapeHtml(formatSpecialMetric("cost", validation.cost))}</span>
        <span><strong>Longueur</strong> ${escapeHtml(formatSpecialMetric("path_len", validation.path_len))}</span>
        <span>${stable ? "Cohérent entre perf et instrumenté" : "Écart entre perf et instrumenté"}</span>
        ${sameAsBaseline ? `<span>Résultat identique à ${escapeHtml(baselineAlgoLabel(true, response))} (${escapeHtml(baselineComparisonTag(response))})</span>` : ""}
      </div>

      <h3>Configuration</h3>
      <div class="result-config-grid">
        ${resultConfigItem("Dataset", selected.dataset_label + " (" + selected.dataset_id + ")")}
        ${resultConfigItem("Scénario", scenario ? scenario.name : q.case_id)}
        ${resultConfigItem("Algorithme", benchAlgorithmDescriptor(q.algo)?.label || q.algo)}
        ${resultConfigItem(baselineReferenceLabel(response), baselineAlgoLabel(false, response))}
        ${q.bucket_state ? resultConfigItem("Données", q.bucket_state === "decoupled" ? "Séparées" : "Regroupées") : ""}
        ${resultConfigItem("Layout", benchOptionLabel("gps_layout", q.gps_layout))}
        ${resultConfigItem("Profil RAM", q.swap_profile)}
        ${resultConfigItem("Graphe", benchOptionLabel("graph_storage", q.graph_storage))}
        ${resultConfigItem("Voisins", benchOptionLabel("neighbor_access", q.neighbor_access))}
        ${resultConfigItem("Politique d’exploration", benchOptionLabel("exploration_policy", q.exploration_policy))}
        ${resultConfigItem("Heuristique", benchOptionLabel("heuristic", q.heuristic))}
        ${resultConfigItem("GPS", selected.gps_file || "—")}
        ${resultConfigItem("CSV", selected.csv)}
        ${resultConfigItem("Exécution", (selected.run_ids || []).join(", ") || "—")}
        ${resultConfigItem("Horodatage", (selected.timestamps || []).join(", ") || "—")}
        ${resultConfigItem("Données brutes", (selected.raw_dirs || []).join(", ") || "—")}
      </div>
      ${resultAlgorithmNotes(q).map((note) => `<p class="metric-coverage-note">${escapeHtml(note)}</p>`).join("")}
      <p class="metric-coverage-note">Le temps de référence provient de l’exécution perf. Toutes les autres métriques affichées proviennent de l’exécution instrumentée correspondante ; les deux modes ne sont jamais moyennés ensemble.</p>
    `;
  return section;
}

function resultAlgorithmNotes(query) {
  const descriptor = benchAlgorithmDescriptor(query.algo);
  return [...new Set([
    descriptor?.description,
    descriptor?.policy_notes?.[query.exploration_policy],
    descriptor?.structure_notes?.[query.bucket_state] || descriptor?.structure_note,
  ].filter(Boolean))];
}

function resultConfigItem(label, value) {
  return `
      <div class="result-config-item">
        <span>${escapeHtml(label)}</span>
        <strong>${escapeHtml(value == null ? "—" : value)}</strong>
      </div>
    `;
}

function shellQuote(value) {
  const text = String(value ?? "");
  if (/^[A-Za-z0-9_./:+-]+$/.test(text)) return text;
  return `'${text.replace(/'/g, `'\"'\"'`)}'`;
}

function benchmarkCommandArgs(response, mode) {
  const q = response?.query || {};
  const scenario = response?.scenario || {};
  const selected = response?.selected || {};
  const args = [
    q.algo || "astar",
    "--mode", mode,
    "--profile", q.swap_profile || "256_128",
    "--layout", q.gps_layout || "base",
    "--case", q.case_id || scenario.name || "case_001",
    "--graph-storage", q.graph_storage || "tile_dicho",
    "--neighbor-access", q.neighbor_access || "tile_segments",
    "--policy", q.exploration_policy || "strict",
  ];

  if (q.bucket_state) args.push("--state", q.bucket_state);

  const file = selected.gps_file || scenario.file;
  if (file) args.push("--file", file);

  const coordinates = [
    Number(scenario.start_lon),
    Number(scenario.start_lat),
    Number(scenario.end_lon),
    Number(scenario.end_lat),
  ];
  if (coordinates.every(Number.isFinite)) {
    args.push(
      "--from", String(coordinates[0]), String(coordinates[1]),
      "--to", String(coordinates[2]), String(coordinates[3]),
    );
  }

  return args.map(shellQuote).join(" ");
}

function benchmarkPerfCommand(response) {
  return `cargo run --release --bin bench-bin -- ${benchmarkCommandArgs(response, "perf")}`;
}

function benchmarkSaveCommand(response) {
  const perf = `scripts/bench.sh run ${benchmarkCommandArgs(response, "perf")}`;
  const inspect = `scripts/bench.sh run ${benchmarkCommandArgs(response, "inspect")}`;
  return `${perf} && ${inspect} && scripts/bench.sh rebuild`;
}

async function copyCommand(button) {
  const command = button?.dataset?.copyCommand || "";
  if (!command) return;

  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(command);
  } else {
    const textarea = document.createElement("textarea");
    textarea.value = command;
    textarea.setAttribute("readonly", "");
    textarea.style.position = "fixed";
    textarea.style.opacity = "0";
    document.body.appendChild(textarea);
    textarea.select();
    document.execCommand("copy");
    textarea.remove();
  }

  const hint = button.querySelector(".result-copy-hint");
  if (!hint) return;
  const previous = hint.textContent;
  hint.textContent = "Copié";
  window.setTimeout(() => {
    if (hint.isConnected) hint.textContent = previous;
  }, 1200);
}

function installCommandCopyHandlers(root) {
  root.querySelectorAll("[data-copy-command]").forEach((button) => {
    button.addEventListener("click", () => {
      copyCommand(button).catch((error) => {
        console.error(error);
        setStatus("Impossible de copier la commande.");
      });
    });
  });
}

function resultCommandRow(label, note, command) {
  return `
      <button type="button" class="result-copy-command" data-copy-command="${escapeHtml(command)}">
        <span class="result-copy-meta">
          <strong>${escapeHtml(label)}</strong>
          <small>${escapeHtml(note)}</small>
        </span>
        <code>${escapeHtml(command)}</code>
        <span class="result-copy-hint">Copier</span>
      </button>
    `;
}

function resultRunCommandsSection(response) {
  const section = document.createElement("section");
  section.className = "result-run-commands modal-span-all";
  section.innerHTML = `
      ${resultCommandRow(
    "Tester la durée perf sur ce PC",
    "Même configuration, aucune sauvegarde dans bench/raw.",
    benchmarkPerfCommand(response),
  )}
      ${resultCommandRow(
    "Regénérer les données de ce run",
    "Perf + instrumenté sauvegardés dans bench/raw, puis reconstruction des résultats.",
    benchmarkSaveCommand(response),
  )}
    `;
  installCommandCopyHandlers(section);
  return section;
}

function memoryComparisonSection(aggregate, baseline) {
  const reads = metricOf(aggregate, "search_flash_reads");
  const writes = metricOf(aggregate, "search_flash_writes");
  const baseReads = metricOf(baseline, "search_flash_reads");
  const baseWrites = metricOf(baseline, "search_flash_writes");

  const section = document.createElement("section");
  section.className = "modal-section modal-span-all memory-comparison-section";
  section.innerHTML = `
      <h3>${escapeHtml(baselineComparisonTitle())}</h3>
      <div class="memory-compare-grid">
        ${memoryComparisonMetricHtml("Lectures flash", reads, baseReads, "search_flash_reads")}
        ${memoryComparisonMetricHtml("Écritures flash", writes, baseWrites, "search_flash_writes")}
      </div>
      <p class="metric-coverage-note">Plus bas = moins d’E/S flash. Une lecture flash correspond exactement à un défaut de cache ; les écritures sont analysées séparément ci-dessous.</p>
    `;
  return section;
}

function cacheOperationRows(aggregate) {
  return PHASE_PREFIXES.map(([prefix, label]) => {
    const calls = metricOf(aggregate, prefix + "_calls") || 0;
    const hits = metricOf(aggregate, prefix + "_cache_hits") || 0;
    const reads = metricOf(aggregate, prefix + "_flash_reads") || 0;
    const writes = metricOf(aggregate, prefix + "_flash_writes") || 0;
    const accesses = hits + reads;
    return {
      prefix,
      label: phaseLabel(prefix, label),
      calls,
      hits,
      reads,
      writes,
      accesses,
      missRate: accesses > 0 ? reads / accesses : null,
      writesPerKCalls: calls > 0 ? (writes / calls) * 1000 : null,
    };
  });
}

function ioDistributionRows(aggregate, valueKey, globalKey) {
  let rows = cacheOperationRows(aggregate)
    .map((row) => ({ label: row.label, value: numberOrZero(row[valueKey]) }))
    .filter((row) => row.value > 0)
    .sort((a, b) => b.value - a.value);

  const globalTotal = metricOf(aggregate, globalKey);
  const attributed = rows.reduce((sum, row) => sum + row.value, 0);
  if (globalTotal != null && globalTotal - attributed > 0.5) {
    rows.push({ label: "Autres / non attribué", value: globalTotal - attributed });
  }

  return {
    rows,
    total: globalTotal != null ? Number(globalTotal) : attributed,
  };
}

function flashReadDistributionSection(aggregate) {
  const distribution = ioDistributionRows(aggregate, "reads", "search_flash_reads");
  const section = document.createElement("section");
  section.className = "modal-section";
  section.innerHTML = `
      <h3>Lectures flash par opération</h3>
      ${distribution.rows.length === 0 ? `<div class="modal-empty">Aucune lecture flash attribuée.</div>` : `
        <div class="modal-chart-grid compact io-donut-grid">
          ${donutHtml(distribution.rows, `${fmt(distribution.total, 0)} lectures`, { legendMode: "count-percent" })}
        </div>
      `}
      <p class="metric-coverage-note">Chaque secteur représente la part des lectures flash déclenchées pendant l’opération correspondante. Une lecture flash équivaut ici à un défaut de cache.</p>
    `;
  return section;
}

function flashWriteDistributionSection(aggregate) {
  const distribution = ioDistributionRows(aggregate, "writes", "search_flash_writes");
  const section = document.createElement("section");
  section.className = "modal-section";
  section.innerHTML = `
      <h3>Écritures flash par opération</h3>
      ${distribution.rows.length === 0 ? `<div class="modal-empty">Aucune écriture flash attribuée.</div>` : `
        <div class="modal-chart-grid compact io-donut-grid">
          ${donutHtml(distribution.rows, `${fmt(distribution.total, 0)} écritures`, { legendMode: "count-percent" })}
        </div>
      `}
      <p class="metric-coverage-note">Chaque secteur représente la part des écritures flash déclenchées pendant l’opération correspondante.</p>
    `;
  return section;
}

function niceScaleCeil(value) {
  if (!(value > 0)) return 1;
  const power = 10 ** Math.floor(Math.log10(value));
  const normalized = value / power;
  const step = normalized <= 1 ? 1 : normalized <= 2 ? 2 : normalized <= 5 ? 5 : 10;
  return step * power;
}

function cacheMissRateScale(rows) {
  const maxRate = rows.reduce((max, row) => Math.max(max, row.missRate || 0), 0);
  const factor = maxRate >= 0.001 ? 1000 : 10000;
  const symbol = factor === 1000 ? "‰" : "‱";
  const maximum = niceScaleCeil(maxRate * factor);
  return { factor, symbol, maximum };
}

function cacheMissRateByOperationSection(aggregate) {
  const rows = cacheOperationRows(aggregate)
    .filter((row) => row.reads > 0 && row.missRate != null)
    .sort((a, b) => b.missRate - a.missRate);
  const scale = cacheMissRateScale(rows);

  const section = document.createElement("section");
  section.className = "modal-section";
  section.innerHTML = `
      <h3>Taux de défaut cache par opération</h3>
      ${rows.length === 0 ? `<div class="modal-empty">Aucun taux de défaut disponible.</div>` : `
        <div class="cache-rate-axis" aria-hidden="true">
          <div class="cache-rate-axis-scale">
            <span>0</span>
            <span>${escapeHtml(fmt(scale.maximum / 2, 1))} ${scale.symbol}</span>
            <span>${escapeHtml(fmt(scale.maximum, 1))} ${scale.symbol}</span>
          </div>
        </div>
        <div class="cache-rate-list">
          ${rows.map((row) => {
    const scaled = row.missRate * scale.factor;
    const width = scale.maximum > 0 ? clamp((scaled / scale.maximum) * 100, 0, 100) : 0;
    return `
              <div class="cache-rate-row">
                <strong>${escapeHtml(row.label)}</strong>
                <div class="theme-bar-track"><div class="theme-bar-fill" style="width:${width}%"></div></div>
                <small>${escapeHtml(fmt(scaled, 2))} ${scale.symbol}</small>
              </div>
            `;
  }).join("")}
        </div>
      `}
      <p class="metric-coverage-note">Le taux mesure les lectures flash rapportées aux accès cache de chaque opération : lectures / (succès cache + lectures). La longueur de la barre représente exactement ce taux.</p>
    `;
  return section;
}

function flashWritesPerKByOperationSection(aggregate) {
  const rows = cacheOperationRows(aggregate)
    .filter((row) => row.writes > 0 && row.writesPerKCalls != null)
    .sort((a, b) => b.writesPerKCalls - a.writesPerKCalls);
  const maximum = niceScaleCeil(rows.reduce((max, row) => Math.max(max, row.writesPerKCalls || 0), 0));

  const section = document.createElement("section");
  section.className = "modal-section";
  section.innerHTML = `
      <h3>Écritures flash pour 1 000 appels</h3>
      ${rows.length === 0 ? `<div class="modal-empty">Aucune écriture flash attribuée à une opération appelée.</div>` : `
        <div class="cache-rate-axis" aria-hidden="true">
          <div class="cache-rate-axis-scale">
            <span>0</span>
            <span>${escapeHtml(fmt(maximum / 2, maximum < 10 ? 2 : 1))}</span>
            <span>${escapeHtml(fmt(maximum, maximum < 10 ? 2 : 1))}</span>
          </div>
        </div>
        <div class="cache-rate-list">
          ${rows.map((row) => {
    const width = maximum > 0 ? clamp((row.writesPerKCalls / maximum) * 100, 0, 100) : 0;
    return `
              <div class="cache-rate-row">
                <strong>${escapeHtml(row.label)}</strong>
                <div class="theme-bar-track"><div class="theme-bar-fill" style="width:${width}%"></div></div>
                <small>${escapeHtml(fmt(row.writesPerKCalls, row.writesPerKCalls < 10 ? 2 : 1))}</small>
              </div>
            `;
  }).join("")}
        </div>
      `}
      <p class="metric-coverage-note">Cette normalisation rapporte les écritures flash au nombre d’appels de l’opération. Elle permet de distinguer une opération très fréquente qui écrit rarement d’une opération peu fréquente mais coûteuse en écritures.</p>
    `;
  return section;
}

const CACHE_MATRIX_ROWS = [
  { key: "calls", label: "Appels", kind: "count" },
  { key: "accesses", label: "Accès cache", kind: "count" },
  { key: "hits", label: "Succès cache", kind: "count" },
  { key: "reads", label: "Lectures flash", kind: "count" },
  { key: "missRate", label: "Taux de défaut cache", kind: "small-rate" },
  { key: "writes", label: "Écritures flash", kind: "count" },
  { key: "writesPerKCalls", label: "Écritures / 1 000 appels", kind: "rate" },
];

function formatCacheMatrixValue(value, kind) {
  if (value == null || !Number.isFinite(Number(value))) return "—";
  const numeric = Number(value);
  if (kind === "small-rate") {
    if (Math.abs(numeric) >= 0.001) return fmt(numeric * 1000, 2) + " ‰";
    return fmt(numeric * 10000, 2) + " ‱";
  }
  if (kind === "rate") return fmt(numeric, Math.abs(numeric) < 10 ? 2 : 1);
  return fmt(numeric, Math.abs(numeric) < 10 && !Number.isInteger(numeric) ? 2 : 0);
}

function cacheOperationMatrixSection(aggregate) {
  const operations = cacheOperationRows(aggregate)
    .filter((row) => row.calls > 0 || row.accesses > 0 || row.writes > 0);

  const visibleRows = CACHE_MATRIX_ROWS.filter((metric) => operations.some((operation) => {
    const value = operation[metric.key];
    return value != null && (numberOrZero(value) !== 0 || metric.key === "missRate" || metric.key === "writesPerKCalls");
  }));

  const section = document.createElement("section");
  section.className = "modal-section";
  section.innerHTML = `
      <h3>Lecture / écriture par opération</h3>
      ${operations.length === 0 ? `<div class="modal-empty">Aucune métrique par opération disponible.</div>` : `
        <div class="cache-matrix-wrap">
          <table class="modal-table cache-matrix-table">
            <thead>
              <tr>
                <th>Mesure</th>
                ${operations.map((operation) => `<th>${escapeHtml(operation.label)}</th>`).join("")}
              </tr>
            </thead>
            <tbody>
              ${visibleRows.map((metric) => `
                <tr>
                  <th>${escapeHtml(metric.label)}</th>
                  ${operations.map((operation) => `<td>${escapeHtml(formatCacheMatrixValue(operation[metric.key], metric.kind))}</td>`).join("")}
                </tr>
              `).join("")}
            </tbody>
          </table>
        </div>
      `}
      <p class="metric-coverage-note">Toutes les valeurs proviennent de l’exécution instrumentée. Les lectures flash sont les défauts de cache ; elles sont rapportées aux accès cache. Les écritures flash sont rapportées au nombre d’appels de l’opération.</p>
    `;
  return section;
}

function phaseRowHtml(label, value, total, shownValue) {
  const width = total > 0 ? clamp((value / total) * 100, 0, 100) : 0;
  return `
      <div class="phase-row">
        <strong>${escapeHtml(label)}</strong>
        <div class="theme-bar-track"><div class="theme-bar-fill" style="width:${width}%"></div></div>
        <small>${escapeHtml(shownValue)}</small>
      </div>
    `;
}

function donutHtml(rows, centerLabel, options = {}) {
  const total = rows.reduce((sum, row) => sum + numberOrZero(row.value), 0);
  const colors = ["#60a5fa", "#4ade80", "#fbbf24", "#fb7185", "#a78bfa", "#22d3ee", "#f97316", "#94a3b8", "#e879f9", "#34d399", "#c084fc", "#fda4af"];
  let cursor = 0;
  const parts = [];

  rows.forEach((row, index) => {
    const value = numberOrZero(row.value);
    if (!total || value <= 0) return;

    const start = cursor;
    const end = cursor + (value / total) * 100;
    cursor = end;

    parts.push(`${colors[index % colors.length]} ${start}% ${end}%`);
  });

  const bg = parts.length
    ? `conic-gradient(${parts.join(", ")})`
    : "conic-gradient(rgba(148, 163, 184, .22) 100%, transparent 0)";

  return `
    <div class="modal-donut" style="--donut-bg:${escapeHtml(bg)}" data-label="${escapeHtml(centerLabel || "—")}"></div>
    ${legendHtml(rows, colors, options.legendMode)}
  `;
}

function legendHtml(rows, colors, mode = "percent") {
  const total = rows.reduce((sum, row) => sum + numberOrZero(row.value), 0) || 1;

  return `
    <div class="modal-legend">
      ${rows.map((row, index) => {
    const value = numberOrZero(row.value);
    const pct = (value / total) * 100;
    const permille = (value / total) * 1000;

    let valueText;

    if (mode === "count-percent") {
      valueText = `${fmt(value, 0)} · ${fmt(pct, 1)} %`;
    } else if (mode === "permille") {
      valueText = fmtPermille(permille);
    } else {
      valueText = `${fmt(pct, 1)} %`;
    }

    return `
          <div class="modal-legend-row" style="--swatch:${colors ? colors[index % colors.length] : "#60a5fa"}">
            <span class="modal-legend-swatch"></span>
            <span>${escapeHtml(row.label)}</span>
            <strong>${escapeHtml(valueText)}</strong>
          </div>
        `;
  }).join("")}
    </div>
  `;
}

function fmtPermille(value) {
  if (value == null || !Number.isFinite(value)) return "—";
  const decimals = value >= 100 ? 1 : value >= 10 ? 2 : 3;
  return `${fmt(value, decimals)}‰`;
}

function timePhaseSection(aggregate) {
  return phaseFlashSection(aggregate, "Durée E/S projetée par opération", "projected");
}

function workPairedBarHtml(label, metricKey, selectedValue, baselineValue, better = "lower") {
  const selected = Number(selectedValue);
  const baseline = Number(baselineValue);
  const hasSelected = selectedValue != null && selectedValue !== "" && Number.isFinite(selected);
  const hasBaseline = baselineValue != null && baselineValue !== "" && Number.isFinite(baseline);
  const maximum = hasSelected || hasBaseline ? Math.max(hasSelected ? selected : 0, hasBaseline ? baseline : 0, 1) : 1;
  const selectedWidth = hasSelected ? clamp((selected / maximum) * 100, 0, 100) : 0;
  const baselineWidth = hasBaseline ? clamp((baseline / maximum) * 100, 0, 100) : 0;
  const kind = workDeltaKind(selectedValue, baselineValue, better);

  return `
      <div class="work-compare-row">
        <div class="work-compare-title">
          <strong>${escapeHtml(label)}</strong>
          <span class="work-compare-delta ${kind ? `metric-tone-${kind}` : ""}">${escapeHtml(signedMetricDeltaHtmlValue(metricKey, selectedValue, baselineValue))}</span>
        </div>
        <div class="work-compare-line">
          <span>${escapeHtml(workAlgoLabel(state.benchSelection?.algo, true))}</span>
          <div class="work-compare-track"><div class="work-compare-fill selected" style="width:${selectedWidth}%"></div></div>
          <strong>${escapeHtml(fmtMetric(metricKey, selectedValue))}</strong>
        </div>
        <div class="work-compare-line">
          <span>${escapeHtml(baselineAlgoLabel(true))}</span>
          <div class="work-compare-track"><div class="work-compare-fill baseline" style="width:${baselineWidth}%"></div></div>
          <strong>${escapeHtml(fmtMetric(metricKey, baselineValue))}</strong>
        </div>
      </div>
    `;
}

function workAlgoLabel(algo, short = false) {
  const [raw, variantState, variantPolicy] = String(algo || "").split("__");
  const descriptor = benchAlgorithmDescriptor(raw);
  if (!descriptor) return raw || "Sélection";
  const label = short ? descriptor.short_label : descriptor.label;
  if (descriptor.states.length <= 1 && descriptor.policies.length <= 1) return label;
  const stateMode = variantState || state.benchSelection?.bucket_state || descriptor.states[0];
  const policy = variantPolicy || state.benchSelection?.exploration_policy || descriptor.policies[0];
  const configurableState = descriptor.states.length > 1;
  const stateLabel = configurableState ? (stateMode === "coupled" ? "couplé" : stateMode === "decoupled" ? "découplé" : stateMode) : "";
  const policyLabel = benchOptionLabel("exploration_policy", policy).toLowerCase();
  const stateShort = configurableState ? (stateMode === "coupled" ? "C" : stateMode === "decoupled" ? "D" : stateMode) : "";
  const policyShort = policy === "strict" ? "S" : policy === "tile_aware" ? "T" : policy === "spatial_tiebreak" ? "X" : policy === "spatial_priority" ? "P" : policy;
  return short ? `${label}-${stateShort || ""}${policyShort}` : `${label}${stateLabel ? ` · ${stateLabel}` : ""} · ${policyLabel}`;
}

function workAlgorithmLegend() {
  return (state.benchConfig?.algorithms || []).filter((entry) => entry.enabled)
    .map((entry) => `${entry.short_label} = ${entry.label}`).join(", ")
    + ". S = strict, T = tile-aware, X = départage spatial, P = priorité spatiale ; C = couplé, D = découplé.";
}

function baselineAlgoId(response = state.benchLastResponse) {
  return normalizeBenchAlgoId(response?.baseline_query?.algo || state.benchSelection.baseline_algo);
}

function comparisonAlgoLabel(algo, selection, short = false) {
  if (hasStateOptions(algo) && !selection?.bucket_state) {
    const descriptor = benchAlgorithmDescriptor(algo);
    return (short ? descriptor?.short_label : descriptor?.label) || algo;
  }
  return workAlgoLabel(bucketVariantKey({ ...selection, algo }), short);
}

function baselineAlgoLabel(short = false, response = state.benchLastResponse) {
  return comparisonAlgoLabel(baselineAlgoId(response), response?.baseline_query || state.benchSelection, short);
}

function baselineComparisonTag(response = state.benchLastResponse) {
  return response?.baseline_mode === "legacy_strict_fallback" ? "équivalent strict legacy" : "TCEPA";
}

function baselineReferenceLabel(response = state.benchLastResponse) {
  return `Référence (${baselineComparisonTag(response)})`;
}

function baselineComparisonTitle(response = state.benchLastResponse) {
  return `Comparaison ${baselineAlgoLabel(true, response)} (${baselineComparisonTag(response)})`;
}

function workRawDeltaRowHtml(label, key, selectedValue, baselineValue, scale, baselineLabel) {
  const selected = Number(selectedValue);
  const baseline = Number(baselineValue);
  const hasValues = selectedValue != null && baselineValue != null
    && Number.isFinite(selected) && Number.isFinite(baseline);
  const relative = hasValues && baseline !== 0 ? (selected - baseline) / baseline : null;
  const magnitude = relative == null ? 0 : Math.min(Math.abs(relative) / scale, 1) * 50;
  const side = relative == null || relative === 0 ? "zero" : relative > 0 ? "positive" : "negative";
  const deltaText = relative == null ? "—" : `${relative > 0 ? "+" : ""}${fmt(relative * 100, 1)} %`;
  return `
      <div class="work-delta-row">
        <strong>${escapeHtml(label)}</strong>
        <div class="work-delta-track">
          <i class="work-delta-zero"></i>
          <i class="work-delta-fill ${side}" style="width:${magnitude}%;"></i>
        </div>
        <span class="work-delta-value ${side}">${escapeHtml(deltaText)}</span>
        <small>${escapeHtml(fmtMetric(key, selectedValue))} · ${escapeHtml(baselineLabel)} ${escapeHtml(fmtMetric(key, baselineValue))}</small>
      </div>
    `;
}

function workRawComparisonSection(selected, baseline, response) {
  const baselineLabel = baselineAlgoLabel(true, response);
  const rows = [
    ["Appels instrumentés", "algorithmic_work"],
    ["Graphe · neighbors open/next", "algorithmic_work_graph"],
    ["Frontière · heap/global/local", "algorithmic_work_frontier"],
    ["État · done/previous/states", "algorithmic_work_state"],
    ["Heuristique · calls", "algorithmic_work_heuristic"],
  ];
  const relativeValues = rows.map(([, key]) => {
    const value = Number(metricOf(selected, key));
    const base = Number(metricOf(baseline, key));
    return Number.isFinite(value) && Number.isFinite(base) && base !== 0 ? Math.abs((value - base) / base) : 0;
  });
  const scale = Math.max(0.01, ...relativeValues);
  const section = document.createElement("section");
  section.className = "modal-section";
  section.innerHTML = `
      <h3>Appels instrumentés · écart à ${escapeHtml(baselineLabel)}</h3>
      <div class="work-delta-chart">
        <div class="work-delta-axis"><span>moins</span><strong>0 = ${escapeHtml(baselineLabel)}</strong><span>plus</span></div>
        ${rows.map(([label, key]) => workRawDeltaRowHtml(label, key, metricOf(selected, key), metricOf(baseline, key), scale, baselineLabel)).join("")}
      </div>
      <div class="work-inline-kpis">
        ${modalKpiHtml("Visited / path", fmtMetric("visited_per_path_node", metricOf(selected, "visited_per_path_node")))}
        ${modalKpiHtml("Heuristic / node", fmtMetric("heuristic_per_visited_node", metricOf(selected, "heuristic_per_visited_node")))}
      </div>
      <p class="metric-coverage-note">Chaque ligne compare le nombre d'appels instrumentés à la référence TCEPA sélectionnée. Le total additionne les familles graphe, frontière, état et heuristique sans ajouter edges_visited, graph_edge_work ou les métriques spécifiques. Il décrit un volume d'activité instrumentée ; il ne suppose pas qu'un appel d'heuristique, d'arbre ou de fenêtre possède le même coût CPU.</p>
    `;
  return section;
}

function workPartitionRowHtml(label, keptLabel, skippedLabel, kept, skipped, baseKept, baseSkipped) {
  const keptValue = numberOrZero(kept);
  const skippedValue = numberOrZero(skipped);
  const total = keptValue + skippedValue;
  const skippedRate = total > 0 ? skippedValue / total : null;

  const baseKeptValue = numberOrZero(baseKept);
  const baseSkippedValue = numberOrZero(baseSkipped);
  const baseTotal = baseKeptValue + baseSkippedValue;
  const baseSkippedRate = baseTotal > 0 ? baseSkippedValue / baseTotal : null;

  const bar = (rowLabel, rowKept, rowSkipped, rowTotal, rowRate) => {
    const keptWidth = rowTotal > 0 ? clamp((rowKept / rowTotal) * 100, 0, 100) : 0;
    const skippedWidth = rowTotal > 0 ? clamp((rowSkipped / rowTotal) * 100, 0, 100) : 0;
    return `
        <div class="work-partition-line">
          <span>${escapeHtml(rowLabel)}</span>
          <div class="work-partition-track" title="${escapeHtml(keptLabel)} ${escapeHtml(fmt(rowKept))} · ${escapeHtml(skippedLabel)} ${escapeHtml(fmt(rowSkipped))}">
            <div class="work-partition-kept" style="width:${keptWidth}%"></div>
            <div class="work-partition-skipped" style="width:${skippedWidth}%"></div>
          </div>
          <strong>${rowRate == null ? "—" : escapeHtml(fmt(rowRate * 100, 1) + " % skipped")}</strong>
        </div>
      `;
  };

  return `
      <div class="work-prune-panel">
        <div class="work-prune-panel-title"><strong>${escapeHtml(label)}</strong></div>
        <div class="work-partition-legend">
          <span><i class="kept"></i>${escapeHtml(keptLabel)}</span>
          <span><i class="skipped"></i>${escapeHtml(skippedLabel)}</span>
        </div>
        ${bar(workAlgoLabel(state.benchSelection?.algo, true), keptValue, skippedValue, total, skippedRate)}
        ${bar(baselineAlgoLabel(true), baseKeptValue, baseSkippedValue, baseTotal, baseSkippedRate)}
      </div>
    `;
}

function workPruningSection(selected, baseline) {
  const section = document.createElement("section");
  section.className = "modal-section";
  section.innerHTML = `
      <h3>Travail pruned</h3>
      <div class="work-prune-grid">
        ${workPartitionRowHtml(
    "Neighbor pruning",
    "pushed",
    "skipped",
    metricOf(selected, "neighbors_visited"),
    metricOf(selected, "neighbors_skipped"),
    metricOf(baseline, "neighbors_visited"),
    metricOf(baseline, "neighbors_skipped"),
  )}
        ${workPartitionRowHtml(
    "Pop outcome",
    "visited",
    "skipped",
    metricOf(selected, "nodes_visited"),
    metricOf(selected, "nodes_skipped"),
    metricOf(baseline, "nodes_visited"),
    metricOf(baseline, "nodes_skipped"),
  )}
      </div>
      <p class="metric-coverage-note">Neighbor pruning montre les candidats éliminés avant un push. Pop outcome montre au contraire le travail déjà entré dans la file de candidats puis rejeté au pop. Les deux barres sont de vraies partitions à 100 %, pas une somme artificielle de compteurs hétérogènes.</p>
    `;
  return section;
}

function collectionPeakRows(algo) {
  const declared = benchAlgorithmDescriptor(algo)?.collections;
  if (declared?.length) return declared.filter((entry) => !entry.state || entry.state === state.benchSelection.bucket_state);

  if (isCountdownAlgo(algo) && usesDecoupledState()) {
    return [
      { label: "Heap", key: "max_heap_bytes" },
      { label: "Done", key: "max_done_bytes" },
      { label: "Cost", key: "max_states_bytes", group: "État" },
      { label: "Previous", key: "max_previous_bytes", group: "État" },
    ];
  }
  if (isBucketAlgo(algo)) {
    if (isBucketDecoupled()) {
      return [
        { label: "Global", key: "max_global_bytes" },
        { label: "Local", key: "max_local_bytes" },
        { label: "Cost", key: "max_states_bytes", group: "État" },
        { label: "Previous", key: "max_previous_bytes", group: "État" },
      ];
    }
    return [
      { label: "Global", key: "max_global_bytes" },
      { label: "Local", key: "max_local_bytes" },
      { label: "State (Cost + Previous)", key: "max_states_bytes", group: "État" },
    ];
  }
  return [
    { label: isCountdownAlgo(algo) ? "Frontière (Heap + Cost)" : "Heap", key: "max_heap_bytes" },
    { label: "Done", key: "max_done_bytes" },
    { label: "Previous", key: "max_previous_bytes" },
  ];
}

function collectionStructureNote() {
  const algo = state.benchSelection?.algo;
  const descriptor = benchAlgorithmDescriptor(algo);
  const note = descriptor?.structure_notes?.[state.benchSelection.bucket_state] || descriptor?.structure_note;
  if (note) return note;
  if (isCountdownAlgo(algo) && usesDecoupledState()) return "Countdown découplé : Cost conserve le coût et le compteur des candidats courants ; Previous conserve les prédécesseurs. Les coûts sont supprimés au traitement du nœud.";
  if (isCountdownAlgo(algo)) return "Countdown couplé : chaque entrée de Heap transporte le coût et le prédécesseur. Previous est renseigné au traitement du nœud.";
  if (isBucketAlgo(algo) && isBucketDecoupled()) return "A* Bucket découplé : Cost et Previous séparent l’état chaud du prédécesseur persistant. Les barres sont exprimées en bytes.";
  if (isBucketAlgo(algo)) return "A* Bucket couplé : State (Cost + Previous) contient le coût et le prédécesseur dans une même entrée. Les barres sont exprimées en bytes.";
  return "Heap porte la file de candidats ; Done et Previous sont suivies séparément. Les tailles sont calculées depuis les maxima déjà instrumentés et la taille réelle d'une entrée.";
}

function workCollectionRowHtml(row, maximum, aggregate) {
  const value = metricOf(aggregate, row.key);
  const width = maximum > 0 ? clamp((numberOrZero(value) / maximum) * 100, 0, 100) : 0;
  return `
      <div class="work-collection-row">
        <strong>${escapeHtml(row.label)}</strong>
        <div class="theme-bar-track"><div class="theme-bar-fill" style="width:${width}%"></div></div>
        <span>${escapeHtml(fmtMetric(row.key, value))}</span>
      </div>
    `;
}

function workCollectionsSection(selected, baseline) {
  const algo = state.benchSelection?.algo;
  const rows = collectionPeakRows(algo);
  const maximum = Math.max(1, ...rows.map((row) => numberOrZero(metricOf(selected, row.key))));
  const selectedPeak = metricOf(selected, "max_collection_bytes");
  const baselinePeak = metricOf(baseline, "max_collection_bytes");
  const plainRows = rows.filter((row) => !row.group);
  const groupedNames = [...new Set(rows.filter((row) => row.group).map((row) => row.group))];
  const section = document.createElement("section");
  section.className = "modal-section";
  section.innerHTML = `
      <h3>Contention des collections</h3>
      <div class="work-collection-total">
        ${workPairedBarHtml("Plus grosse collection au peak", "max_collection_bytes", selectedPeak, baselinePeak, "lower")}
      </div>
      <div class="work-collection-list">
        ${plainRows.map((row) => workCollectionRowHtml(row, maximum, selected)).join("")}
        ${groupedNames.map((group) => `
          <div class="work-collection-group">
            <div class="work-collection-group-label">${escapeHtml(group)}</div>
            ${rows.filter((row) => row.group === group).map((row) => workCollectionRowHtml(row, maximum, selected)).join("")}
          </div>
        `).join("")}
      </div>
      <p class="metric-coverage-note">${escapeHtml(collectionStructureNote())} Le peak global affiché est celui de la plus grosse collection individuelle ; les maxima de collections différentes ne sont jamais additionnés artificiellement.</p>
    `;
  return section;
}

const WORK_COMMON_METRICS = [
  "algorithmic_work",
  "algorithmic_work_graph",
  "algorithmic_work_frontier",
  "algorithmic_work_state",
  "algorithmic_work_heuristic",
  "nodes_visited",
  "edges_visited",
  "graph_edge_work",
  "heap_pushes",
  "queue_operations",
  "heuristic_calls",
  "nodes_skipped",
  "neighbors_skipped",
  "pop_skipped_ratio",
  "neighbors_skipped_ratio",
  "visited_per_path_node",
  "heuristic_per_visited_node",
];

function workSpecificMetricKeys() {
  const algo = state.benchSelection?.algo;
  const declared = benchAlgorithmDescriptor(algo)?.specific_metrics;
  if (declared?.length) return [...new Set(["heap_pops", ...collectionPeakRows(algo).map((item) => item.count_key).filter(Boolean), ...declared])];
  if (isBucketAlgo(algo)) {
    const keys = ["global_pushes", "global_pops", "local_pushes", "local_pops", "max_heap", "max_global", "max_local", "max_states", "states_reads", "states_writes"];
    if (isBucketDecoupled()) keys.splice(8, 0, "max_previous");
    return keys;
  }
  if (algo === "astar_countdown") {
    return ["heap_pops", "max_heap", "max_done", "max_previous", "countdown_done_checks", "countdown_done_inserts", "countdown_done_decs", "countdown_done_removes", "countdown_scans", "countdown_edges"];
  }
  return ["heap_pops", "max_heap", "max_done", "max_previous"];
}

function formatMetricDelta(key, value, base) {
  if (value == null || base == null) return "—";
  const delta = Number(value) - Number(base);
  if (!Number.isFinite(delta)) return "—";
  const sign = delta > 0 ? "+" : delta < 0 ? "−" : "";
  const magnitude = Math.abs(delta);
  if (key.endsWith("_bytes")) return sign + fmtBytes(magnitude);
  if (key.endsWith("_rate") || key.endsWith("_ratio")) return sign + fmt(magnitude * 100, 1) + " %";
  if (key.includes("per_")) return sign + fmt(magnitude, 2);
  return sign + fmt(magnitude, magnitude >= 100 ? 0 : magnitude >= 10 ? 1 : 2);
}

function workAlgorithmColumns(response) {
  const selection = response.query || state.benchSelection;
  const selectedId = normalizeBenchAlgoId(selection.algo);
  const canonical = (state.benchConfig?.baseline_algorithms || []).map((option) => option.id);
  const ids = [selectedId, ...canonical.filter((id) => id !== selectedId)];
  const seen = new Set();
  return ids.filter((id) => id && !seen.has(id) && seen.add(id)).map((id) => ({
    id,
    label: comparisonAlgoLabel(id, selection, true),
    full: comparisonAlgoLabel(id, selection),
    selected: id === selectedId,
    aggregate: id === selectedId
      ? response.selected
      : response.work_algorithms?.[id] || (id === baselineAlgoId(response) ? response.baseline : null),
  }));
}

function workCommonMetricsSection(response, astar = astarWorkBaseline(response)) {
  const columns = workAlgorithmColumns(response);
  const baseline = astar;
  const selected = response.selected;
  const section = document.createElement("section");
  section.className = "modal-section";
  section.innerHTML = `
      <h3>Métriques communes</h3>
      <div class="metric-raw-table-wrap work-metric-table-wrap work-common-table-wrap">
        <table class="modal-table work-common-table">
          <thead><tr>
            <th>métrique</th>
            ${columns.map((column) => `<th class="${column.selected ? "work-selected-col" : ""}" title="${escapeHtml(column.full)}">${escapeHtml(column.label)}</th>`).join("")}
            <th>Δ / ${escapeHtml(baselineAlgoLabel(true, response))}</th>
          </tr></thead>
          <tbody>${WORK_COMMON_METRICS.map((key) => `
            <tr>
              <td>${escapeHtml(key)}</td>
              ${columns.map((column) => `<td class="${column.selected ? "work-selected-col" : ""}">${escapeHtml(fmtMetric(key, metricOf(column.aggregate, key)))}</td>`).join("")}
              <td>${escapeHtml(formatMetricDelta(key, metricOf(selected, key), metricOf(baseline, key)))}</td>
            </tr>
          `).join("")}</tbody>
        </table>
      </div>
      <p class="metric-coverage-note">${escapeHtml(workAlgorithmLegend())} Même scénario, profil RAM, layout, graphe et voisins. Les options propres aux algorithmes sont reprises lorsqu’elles s’appliquent. « — » indique une configuration indisponible.</p>
    `;
  return section;
}

function workSpecificMetricsSection(selected) {
  const keys = workSpecificMetricKeys();
  const section = document.createElement("section");
  section.className = "modal-section";
  section.innerHTML = `
      <h3>Métriques spécifiques à l'algo</h3>
      <div class="metric-raw-table-wrap work-metric-table-wrap">
        <table class="modal-table">
          <thead><tr><th>métrique</th><th>valeur</th></tr></thead>
          <tbody>${keys.map((key) => `
            <tr>
              <td>${escapeHtml(metricDisplayKey(key))}</td>
              <td>${escapeHtml(fmtMetric(key, metricOf(selected, key)))}</td>
            </tr>
          `).join("")}</tbody>
        </table>
      </div>
      <p class="metric-coverage-note">${escapeHtml(collectionStructureNote())}</p>
    `;
  return section;
}

function localityRateCompareRowHtml(label, key, selectedValue, baselineValue) {
  const selected = Number(selectedValue);
  const baseline = Number(baselineValue);
  const hasSelected = selectedValue != null && Number.isFinite(selected);
  const hasBaseline = baselineValue != null && Number.isFinite(baseline);
  const kind = workDeltaKind(selectedValue, baselineValue, "lower");
  return `
      <div class="locality-compare-row">
        <div class="locality-compare-head">
          <strong>${escapeHtml(label)}</strong>
          <span class="${kind ? `metric-tone-${kind}` : ""}">${escapeHtml(signedMetricDeltaHtmlValue(key, selectedValue, baselineValue))}</span>
        </div>
        <div class="locality-compare-line">
          <span>${escapeHtml(workAlgoLabel(state.benchSelection?.algo, true))}</span>
          <div class="locality-rate-track"><div class="locality-rate-fill selected" style="width:${hasSelected ? clamp(selected * 100, 0, 100) : 0}%"></div></div>
          <strong>${escapeHtml(fmtMetric(key, selectedValue))}</strong>
        </div>
        <div class="locality-compare-line">
          <span>${escapeHtml(baselineAlgoLabel(true))}</span>
          <div class="locality-rate-track"><div class="locality-rate-fill baseline" style="width:${hasBaseline ? clamp(baseline * 100, 0, 100) : 0}%"></div></div>
          <strong>${escapeHtml(fmtMetric(key, baselineValue))}</strong>
        </div>
      </div>
    `;
}

function localityCrossComparisonSection(selected, baseline) {
  const specs = [
    ["Visited cross", "visited_cross_tile_rate"],
    ["Popped cross", "popped_cross_tile_rate"],
    ["Skipped cross", "skipped_cross_tile_rate"],
    ["Heap push cross", "heap_push_cross_tile_rate"],
    ["Neighbors next cross", "neighbors_next_cross_tile_rate"],
  ].filter(([, key]) => metricOf(selected, key) != null || metricOf(baseline, key) != null);

  const section = document.createElement("section");
  section.className = "modal-section";
  section.innerHTML = `
      <h3>Fréquence des changements de tile</h3>
      <div class="locality-rate-axis" aria-hidden="true"><span>0 %</span><span>50 %</span><span>100 %</span></div>
      <div class="locality-compare-list">
        ${specs.map(([label, key]) => localityRateCompareRowHtml(label, key, metricOf(selected, key), metricOf(baseline, key))).join("")}
      </div>
      <p class="metric-coverage-note">Toutes les lignes partagent la même échelle 0–100 %. Visited mesure le flux réellement traité ; Popped inclut le bruit de la file de candidats ; Skipped isole les pops rejetés ; Neighbors next décrit la tendance structurelle du graphe à produire du cross-tile.</p>
    `;
  return section;
}

function localityJumpRowHtml(label, avgKey, maxKey, selected, baseline, maximum) {
  const selectedAvg = metricOf(selected, avgKey);
  const baselineAvg = metricOf(baseline, avgKey);
  const selectedMax = metricOf(selected, maxKey);
  const baselineMax = metricOf(baseline, maxKey);
  const selectedWidth = maximum > 0 ? clamp((numberOrZero(selectedAvg) / maximum) * 100, 0, 100) : 0;
  const baselineWidth = maximum > 0 ? clamp((numberOrZero(baselineAvg) / maximum) * 100, 0, 100) : 0;
  const kind = workDeltaKind(selectedAvg, baselineAvg, "lower");

  return `
      <div class="locality-jump-row">
        <div class="locality-compare-head">
          <strong>${escapeHtml(label)}</strong>
          <span class="${kind ? `metric-tone-${kind}` : ""}">${escapeHtml(signedMetricDeltaHtmlValue(avgKey, selectedAvg, baselineAvg))}</span>
        </div>
        <div class="locality-compare-line">
          <span>${escapeHtml(workAlgoLabel(state.benchSelection?.algo, true))}</span>
          <div class="locality-rate-track"><div class="locality-rate-fill selected" style="width:${selectedWidth}%"></div></div>
          <strong>${escapeHtml(fmtMetric(avgKey, selectedAvg))}</strong>
        </div>
        <div class="locality-compare-line">
          <span>${escapeHtml(baselineAlgoLabel(true))}</span>
          <div class="locality-rate-track"><div class="locality-rate-fill baseline" style="width:${baselineWidth}%"></div></div>
          <strong>${escapeHtml(fmtMetric(avgKey, baselineAvg))}</strong>
        </div>
        <div class="locality-jump-max">jump max · ${escapeHtml(workAlgoLabel(state.benchSelection?.algo, true))} ${escapeHtml(fmtMetric(maxKey, selectedMax))} · ${escapeHtml(baselineAlgoLabel(true))} ${escapeHtml(fmtMetric(maxKey, baselineMax))}</div>
      </div>
    `;
}

function localityJumpComparisonSection(selected, baseline) {
  const specs = [
    ["Visited", "avg_visited_tile_jump", "visited_tile_jump_max"],
    ["Popped", "avg_popped_tile_jump", "popped_tile_jump_max"],
    ["Skipped", "avg_skipped_tile_jump", "skipped_tile_jump_max"],
  ].filter(([, avgKey]) => metricOf(selected, avgKey) != null || metricOf(baseline, avgKey) != null);
  const maximum = niceScaleCeil(Math.max(1, ...specs.flatMap(([, key]) => [numberOrZero(metricOf(selected, key)), numberOrZero(metricOf(baseline, key))])));

  const section = document.createElement("section");
  section.className = "modal-section";
  section.innerHTML = `
      <h3>Amplitude des jumps cross-tile</h3>
      <div class="locality-jump-axis" aria-hidden="true"><span>0</span><span>${escapeHtml(fmt(maximum / 2, maximum < 10 ? 2 : 1))}</span><span>${escapeHtml(fmt(maximum, maximum < 10 ? 2 : 1))}</span></div>
      <div class="locality-compare-list">
        ${specs.map(([label, avgKey, maxKey]) => localityJumpRowHtml(label, avgKey, maxKey, selected, baseline, maximum)).join("")}
      </div>
      <p class="metric-coverage-note">Le jump moyen est calculé uniquement sur les transitions cross-tile. Il complète leur fréquence : deux algorithmes peuvent changer de tile aussi souvent mais parcourir des distances très différentes. Les valeurs ne sont comparables qu'à layout identique.</p>
    `;
  return section;
}

function localityContextKpiHtml(label, key, selected, baseline) {
  return `
      <div class="locality-context-kpi">
        <span>${escapeHtml(label)}</span>
        <strong>${escapeHtml(fmtMetric(key, metricOf(selected, key)))}</strong>
        <small>${escapeHtml(baselineAlgoLabel(true))} : ${escapeHtml(fmtMetric(key, metricOf(baseline, key)))}</small>
      </div>
    `;
}

function localityGraphContextSection(selected, baseline) {
  const section = document.createElement("section");
  section.className = "modal-section";
  section.innerHTML = `
      <h3>Contexte du graphe</h3>
      <div class="locality-context-grid">
        ${localityContextKpiHtml("Neighbors next cross", "neighbors_next_cross_tile_rate", selected, baseline)}
        ${localityContextKpiHtml("Neighbors open cross", "neighbors_open_cross_rate", selected, baseline)}
        ${localityContextKpiHtml("Edges from border nodes", "edges_from_border_node_rate", selected, baseline)}
      </div>
      <p class="metric-coverage-note">Ces métriques décrivent ce que le voisinage et le tuilage imposent au parcours. Elles servent de contexte à Visited cross : une baisse de Visited cross à contexte voisin indique que le scheduling réordonne effectivement le travail vers davantage de localité.</p>
    `;
  return section;
}

const LOCALITY_COMMON_METRICS = [
  "visited_cross_tile_rate",
  "avg_visited_tile_jump",
  "visited_tile_jump_max",
  "popped_cross_tile_rate",
  "avg_popped_tile_jump",
  "popped_tile_jump_max",
  "skipped_cross_tile_rate",
  "avg_skipped_tile_jump",
  "skipped_tile_jump_max",
  "heap_push_cross_tile_rate",
  "neighbors_next_cross_tile_rate",
  "neighbors_open_cross_rate",
  "edges_from_border_node_rate",
];

function localityAlgorithmsTableSection(response) {
  const columns = workAlgorithmColumns(response);
  const selected = response.selected;
  const baseline = response.baseline;
  const section = document.createElement("section");
  section.className = "modal-section";
  section.innerHTML = `
      <h3>Comparaison des algorithmes</h3>
      <div class="metric-raw-table-wrap locality-algo-table-wrap">
        <table class="modal-table work-common-table locality-algo-table">
          <thead><tr>
            <th>métrique</th>
            ${columns.map((column) => `<th class="${column.selected ? "work-selected-col" : ""}" title="${escapeHtml(column.full)}">${escapeHtml(column.label)}</th>`).join("")}
            <th>Δ / ${escapeHtml(baselineAlgoLabel(true, response))}</th>
          </tr></thead>
          <tbody>${LOCALITY_COMMON_METRICS.map((key) => `
            <tr>
              <td>${escapeHtml(key)}</td>
              ${columns.map((column) => `<td class="${column.selected ? "work-selected-col" : ""}">${escapeHtml(fmtMetric(key, metricOf(column.aggregate, key)))}</td>`).join("")}
              <td>${escapeHtml(formatMetricDelta(key, metricOf(selected, key), metricOf(baseline, key)))}</td>
            </tr>
          `).join("")}</tbody>
        </table>
      </div>
      <p class="metric-coverage-note">${escapeHtml(workAlgorithmLegend())} Même scénario, profil RAM, layout, graphe et voisins. Les options propres aux algorithmes sont reprises lorsqu’elles s’appliquent. « — » indique une configuration indisponible. Les jumps restent des distances dans l'ordre du layout courant.</p>
    `;
  return section;
}

function phaseFlashSection(aggregate, title, mode = "measured") {
  let phases = primaryPhaseBreakdown(aggregate, mode);
  let total = phases.reduce((sum, phase) => sum + numberOrZero(phase.value), 0);
  let note;

  if (mode === "projected") {
    const globalProjected = metricOf(aggregate, "search_flash_estimated_ms");
    if (globalProjected != null) {
      const remainder = globalProjected - total;
      if (remainder > 0.0005) {
        phases = [...phases, { label: "Autres / non attribué", value: remainder }]
          .sort((a, b) => b.value - a.value);
      }
      total = globalProjected;
    }
    note = `Projection à partir des compteurs swap-cache de l’exécution instrumentée : lectures×${FLASH_READ_LATENCY_MS} ms + écritures×${FLASH_WRITE_LATENCY_MS} ms. Le total au centre correspond à la durée E/S projetée globale du run.`;
  } else {
    note = `Temps flash observé sur l'hôte pendant l’exécution instrumentée (*_flash_reads_micros + *_flash_writes_micros). Cette mesure native n'est pas utilisée pour décomposer le temps perf.`;
  }

  const section = document.createElement("section");
  section.className = "modal-section";
  section.innerHTML = `
      <h3>${escapeHtml(title)}</h3>
      ${phases.length === 0 ? `<p class="muted">Pas de données instrumentées disponibles pour cette combinaison.</p>` : `
        <div class="modal-chart-grid wide">
          ${donutHtml(phases, fmt(total, total >= 100 ? 0 : 1) + " ms", { legendMode: "percent" })}
          <div class="phase-list">
            ${phases.slice(0, 12).map((phase) => phaseRowHtml(phase.label, phase.value, total, fmt(phase.value, phase.value >= 100 ? 0 : 1) + " ms")).join("")}
          </div>
        </div>
      `}
      <p class="metric-coverage-note">${escapeHtml(note)}</p>
    `;
  return section;
}

function primaryPhaseBreakdown(aggregate, mode = "measured") {
  return PHASE_PREFIXES
    .map(([prefix, label]) => {
      const measured = metricOf(aggregate, prefix + "_flash_measured_ms") || 0;
      const estimated = metricOf(aggregate, prefix + "_flash_estimated_ms") || 0;
      const reads = metricOf(aggregate, prefix + "_flash_reads") || 0;
      const writes = metricOf(aggregate, prefix + "_flash_writes") || 0;
      return {
        label: phaseLabel(prefix, label),
        value: mode === "projected" ? estimated : measured,
        measured,
        estimated,
        reads,
        writes,
      };
    })
    .filter((phase) => phase.value > 0)
    .sort((a, b) => b.value - a.value);
}

function sourceBadgeHtml(source) {
  if (!source) return "";
  const cls = String(source).toLowerCase().replace(/[^a-z]/g, "");
  return `<span class="metric-source-badge ${escapeHtml(cls)}">${escapeHtml(source)}</span>`;
}

function metricRowHtml(key, selected, baseline) {
  const value = metricOf(selected, key);
  const base = metricOf(baseline, key);
  return `
      <tr>
        <td>${escapeHtml(metricDisplayKey(key))}</td>
        <td>${escapeHtml(fmtMetric(key, value))}</td>
        <td>${escapeHtml(fmtMetric(key, base))}</td>
        <td>${escapeHtml(formatMetricDelta(key, value, base))}</td>
        <td>${sourceBadgeHtml(metricSourceLabel(key))}</td>
      </tr>
    `;
}

function themeMetricsTableSection(theme, selected, baseline) {
  const keys = themeMetricKeys(theme.id, selected);
  const section = document.createElement("section");
  section.className = "modal-section";
  section.innerHTML = `
      <h3>Métriques détaillées</h3>
      <div class="metric-raw-table-wrap">
        <table class="modal-table">
          <thead><tr><th>métrique</th><th>sélection</th><th>référence</th><th>écart</th><th>source</th></tr></thead>
          <tbody>${keys.map((key) => metricRowHtml(key, selected, baseline)).join("")}</tbody>
        </table>
      </div>
      <p class="metric-coverage-note">Source : “perf” = durée de référence ; “instrumenté” = compteur de l’exécution instrumentée ; “instrumenté + projection” = modèle lectures/écritures ; “calculé” = ratio ou grandeur dérivée.</p>
    `;
  return section;
}

function noDataBlock(response) {
  const div = document.createElement("div");
  div.className = "no-data";
  const query = response.query || {};
  div.innerHTML = `
      <strong>No data</strong>
      <p>${escapeHtml(response.message || "Aucune donnée pour cette combinaison.")}</p>
      <pre>${escapeHtml(JSON.stringify(query, null, 2))}</pre>
      <div class="no-data-command">
        ${resultCommandRow(
    "Générer ces données",
    "Perf + instrumenté dans bench/raw, puis reconstruction des résultats.",
    benchmarkSaveCommand(response),
  )}
      </div>
    `;
  installCommandCopyHandlers(div);
  return div;
}

export {
  baselineAlgoLabel,
  baselineComparisonTitle,
  installBenchModalHandlers,
  modalKpiHtml,
  noDataBlock,
  openThemeDetails,
};
