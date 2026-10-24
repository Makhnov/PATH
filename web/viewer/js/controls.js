// Modes, scénarios et options de recherche

import { byId, fetchJson, setStatus } from "./utils.js";
import { state } from "./state.js";
import {
  fetchVisualizeResult,
  pauseVisualizeReplay,
  renderVisualizeSpatialControls,
  saveVisualizeSpatialState,
} from "./replay/player.js";
import { draw, placeCoord, resetCamera, resetLive } from "./map.js";
import { fetchBenchResult } from "./stats/dashboard.js";
import { renderVisualizeCacheTimeline } from "./replay/cache.js";

function renderModeControls() {
  const root = byId("mode-toggle");
  if (!root) return;

  root.innerHTML = "";
  for (const [id, label] of [["stats", "Stats"], ["visualize", "Replay"]]) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "toggle-button" + (state.mode === id ? " active" : "");
    button.textContent = label;
    button.addEventListener("click", () => setMode(id));
    root.appendChild(button);
  }
}

function setMode(mode) {
  if (mode !== "stats" && mode !== "visualize") return;
  if (state.mode === mode) return;

  if (state.mode === "visualize") {
    pauseVisualizeReplay();
    saveVisualizeSpatialState();
  }

  state.mode = mode;
  if (state.mode !== "visualize") resetLive();

  renderModeControls();
  renderModeDependentUi();

  if (state.mode === "stats") {
    fetchBenchResult();
    setStatus("Mode Stats : métriques du scénario sélectionné.");
  } else {
    fetchVisualizeResult();
    setStatus("Mode Visualize : replay instrumenté du scénario sélectionné.");
  }

  requestAnimationFrame(() => {
    resetCamera();
    draw();
  });
}

function benchAlgorithmDescriptor(value) {
  const raw = String(value || "").trim().toLowerCase();
  return (state.benchConfig?.algorithms || []).find((entry) => [entry.id, entry.command].includes(raw));
}

function normalizeBenchAlgoId(value) {
  const entry = benchAlgorithmDescriptor(value);
  return entry?.id || String(value || "astar_lazy").trim().toLowerCase();
}

function isBucketAlgo(value) {
  return benchAlgorithmDescriptor(value)?.ui_model === "bucket";
}

function isCountdownAlgo(value) {
  return benchAlgorithmDescriptor(value)?.ui_model === "countdown";
}

function hasStateOptions(value) {
  return (benchAlgorithmDescriptor(value)?.states || []).length > 0;
}

function isBucketDecoupled() {
  return isBucketAlgo(state.benchSelection?.algo) && state.benchSelection?.bucket_state === "decoupled";
}

function usesDecoupledState() {
  return hasStateOptions(state.benchSelection?.algo) && state.benchSelection?.bucket_state === "decoupled";
}

function bucketVariantKey(selection = state.benchSelection) {
  const algo = normalizeBenchAlgoId(selection?.algo);
  if (!hasStateOptions(algo)) return algo;
  return `${algo}__${selection?.bucket_state || "coupled"}__${selection?.exploration_policy || "strict"}`;
}

function renderModeDependentUi() {
  const stats = state.mode === "stats";
  const visualize = state.mode === "visualize";

  document.body.classList.toggle("bench-mode", stats);
  document.body.classList.toggle("visualize-mode", visualize);

  const controls = byId("bench-controls-card");
  if (controls) controls.hidden = false;
  const baselineField = byId("bench-baseline-field");
  if (baselineField) baselineField.hidden = !stats;

  for (const id of ["bench-main", "bench-side", "bench-details"]) {
    const el = byId(id);
    if (el) el.hidden = !stats;
  }

  const visualizePanel = byId("visualize-panel");
  if (visualizePanel) visualizePanel.hidden = !visualize;
  const cacheTimeline = byId("visualize-cache-timeline");
  if (cacheTimeline) cacheTimeline.hidden = !visualize;

  renderVisualizeSpatialControls();
  renderVisualizeCacheTimeline();
  renderVisualizeLayoutControls();
}

function renderVisualizeLayoutControls() {
  const root = byId("visualize-layout-controls");
  if (!root) return;

  const visible = state.mode === "visualize";
  root.hidden = !visible;

  if (typeof document !== "undefined" && document.body) {
    document.body.classList.toggle(
      "visualize-left-collapsed",
      visible && state.visualizeLeftCollapsed,
    );
    document.body.classList.toggle(
      "visualize-right-collapsed",
      visible && state.visualizeRightCollapsed,
    );
  }

  const left = byId("visualize-toggle-left");
  if (left) {
    const collapsed = state.visualizeLeftCollapsed;
    const label = collapsed ? "Rouvrir le panneau gauche" : "Réduire le panneau gauche";
    left.textContent = collapsed ? "›" : "‹";
    left.title = label;
    left.setAttribute("aria-label", label);
    left.setAttribute("aria-pressed", String(collapsed));
    left.classList.toggle("active", collapsed);
  }

  const right = byId("visualize-toggle-right");
  if (right) {
    const collapsed = state.visualizeRightCollapsed;
    const label = collapsed ? "Rouvrir le panneau droit" : "Réduire le panneau droit";
    right.textContent = collapsed ? "‹" : "›";
    right.title = label;
    right.setAttribute("aria-label", label);
    right.setAttribute("aria-pressed", String(collapsed));
    right.classList.toggle("active", collapsed);
  }
}

function toggleVisualizeSide(side) {
  if (side === "left") {
    state.visualizeLeftCollapsed = !state.visualizeLeftCollapsed;
  } else {
    state.visualizeRightCollapsed = !state.visualizeRightCollapsed;
  }

  renderVisualizeLayoutControls();
  window.requestAnimationFrame(() => draw());
}

function applyBenchOptionCoupling(changedGroupId, previousAlgo) {
  const selection = state.benchSelection;

  const descriptor = benchAlgorithmDescriptor(selection.algo);
  const states = descriptor?.states || [];
  const policies = descriptor?.policies || ["strict"];
  if (!states.includes(selection.bucket_state)) selection.bucket_state = states[0] || "";
  // La politique intrinsèque de l'algorithme précédent n'était pas un choix utilisateur.
  // Partir du préréglage du nouvel algorithme ; conserver les choix explicitement configurables.
  const previousPolicies = previousAlgo ? benchAlgorithmDescriptor(previousAlgo)?.policies : null;
  if ((changedGroupId === "algo" && previousPolicies?.length === 1)
    || !policies.includes(selection.exploration_policy)) selection.exploration_policy = policies[0];
  if (descriptor?.uses_heuristic === false) selection.heuristic = "standard";

  // Dichotomic et Indexed parcourent les mêmes segments sources après la recherche
  // de tuile. Indexed construit une fois un index pour accéder directement à leurs bornes.
  selection.neighbor_access = "tile_segments";
  if (!["tile_dicho", "tile_index"].includes(selection.graph_storage)) selection.graph_storage = "tile_dicho";
}

function isBenchOptionGroupDisabled(groupId) {
  const descriptor = benchAlgorithmDescriptor(state.benchSelection.algo);
  return (groupId === "heuristic" && descriptor?.uses_heuristic === false)
    || (groupId === "bucket_state" && (descriptor?.states || []).length <= 1)
    || (groupId === "exploration_policy" && (descriptor?.policies || []).length <= 1);
}

function benchOptionLabel(groupId, optionId) {
  const declared = (state.benchConfig?.option_groups || []).find((group) => group.id === groupId)
    ?.options?.find((option) => option.id === optionId)?.label;
  if (declared) return declared;
  const policies = {
    strict: "Stricte",
    tile_aware: "Tile-aware",
    spatial_tiebreak: "Départage spatial",
    spatial_priority: "Priorité spatiale",
  };
  return (groupId === "exploration_policy" ? policies[optionId] : null) || optionId || "—";
}

function isBenchOptionDisabled(groupId, optionId) {
  const descriptor = benchAlgorithmDescriptor(state.benchSelection.algo);
  if (isBenchOptionGroupDisabled(groupId)) return true;
  if (groupId === "exploration_policy") return !(descriptor?.policies || []).includes(optionId);
  if (groupId === "bucket_state") return !(descriptor?.states || []).includes(optionId);
  if (groupId === "neighbor_access") return optionId !== "tile_segments";
  return false;
}

function benchScenarioLabel(name) {
  const labels = {
    case_001: "Hypercentre",
    case_002: "Centre ville",
    case_003: "Banlieue (NE-SO)",
    case_004: "Banlieue (N-S)",
    case_005: "Banlieue (E-O)",
  };

  return labels[name] || name;
}

function refreshSelectedMode() {
  if (!state.benchConfig) return;
  if (state.mode === "stats") fetchBenchResult();
  else if (state.mode === "visualize") fetchVisualizeResult();
}

function renderBenchControls() {
  if (!state.benchConfig) return;

  const baselineSelect = byId("bench-baseline-algo");
  if (baselineSelect) {
    baselineSelect.innerHTML = "";
    for (const option of state.benchConfig.baseline_algorithms || []) {
      const el = document.createElement("option");
      el.value = option.id;
      el.textContent = option.label;
      baselineSelect.appendChild(el);
    }
    baselineSelect.value = state.benchSelection.baseline_algo;
    baselineSelect.onchange = () => {
      state.benchSelection.baseline_algo = baselineSelect.value;
      fetchBenchResult();
    };
  }

  const swapSelect = byId("bench-swap-profile");
  if (swapSelect) {
    const swapGroup = (state.benchConfig.option_groups || []).find(
      (group) => group.id === "swap_profile"
    );

    swapSelect.innerHTML = "";

    for (const option of swapGroup?.options || []) {
      const el = document.createElement("option");
      el.value = option.id;
      el.textContent = option.label;
      swapSelect.appendChild(el);
    }

    swapSelect.value = state.benchSelection.swap_profile;
    swapSelect.onchange = () => {
      state.benchSelection.swap_profile = swapSelect.value;
      refreshSelectedMode();
    };
  }

  const scenarioSelect = byId("bench-scenario");
  if (scenarioSelect) {
    scenarioSelect.innerHTML = "";

    for (const scenario of state.benchConfig.scenarios || []) {
      const option = document.createElement("option");
      option.value = scenario.name;
      option.textContent = benchScenarioLabel(scenario.name);
      scenarioSelect.appendChild(option);
    }

    scenarioSelect.value = state.benchSelection.scenario;
    scenarioSelect.onchange = () => {
      state.benchSelection.scenario = scenarioSelect.value;
      pauseVisualizeReplay();
      resetLive();
      applyBenchScenario();
      refreshSelectedMode();
    };
  }

  const optionsRoot = byId("bench-options");
  if (!optionsRoot) return;

  optionsRoot.innerHTML = "";

  for (const group of state.benchConfig.option_groups || []) {
    if (group.id === "swap_profile") continue;
    const block = document.createElement("div");
    block.className = "option-block";

    const title = document.createElement("div");
    title.className = "option-title";
    title.textContent = group.label;
    block.appendChild(title);

    const row = document.createElement("div");
    row.className = "toggle-row wrap";

    for (const option of group.options || []) {
      const current = state.benchSelection[group.id];
      const button = document.createElement("button");
      button.type = "button";
      const disabled = isBenchOptionDisabled(group.id, option.id);

      const groupDisabled = isBenchOptionGroupDisabled(group.id);
      // Une option intrinsèque décrit toujours l'exécution courante lorsque son
      // sélecteur est désactivé (par exemple, l'exploration stricte d'A*).
      const active = current === option.id && (!groupDisabled
        || group.id === "exploration_policy"
        || (group.id === "bucket_state" && hasStateOptions(state.benchSelection.algo)));

      button.className =
        "toggle-button small" +
        (active ? " active" : "") +
        (disabled ? " disabled" : "");

      button.disabled = disabled;
      button.textContent = option.label;

      button.addEventListener("click", () => {
        if (button.disabled) return;

        const previousAlgo = state.benchSelection.algo;
        state.benchSelection[group.id] = option.id;
        applyBenchOptionCoupling(group.id, previousAlgo);

        renderBenchControls();
        refreshSelectedMode();
      });
      row.appendChild(button);
    }

    block.appendChild(row);
    optionsRoot.appendChild(block);
  }

  renderScenarioInfo();
}

function renderScenarioInfo() {
  const root = byId("bench-scenario-info");
  if (!root || !state.benchConfig) return;

  const scenario = currentScenario();

  if (!scenario) {
    root.textContent = "Aucun scénario disponible.";
    return;
  }

  root.innerHTML = `
      <div><strong>${scenario.name}</strong></div>
      <div>fichier: <code>${scenario.file}</code></div>
      <div>départ: ${Number(scenario.start_lon).toFixed(6)}, ${Number(scenario.start_lat).toFixed(6)}</div>
      <div>arrivée: ${Number(scenario.end_lon).toFixed(6)}, ${Number(scenario.end_lat).toFixed(6)}</div>
    `;
}

function currentScenario() {
  if (!state.benchConfig) return null;
  return (state.benchConfig.scenarios || []).find((s) => s.name === state.benchSelection.scenario) || null;
}

function applyBenchScenario() {
  const scenario = currentScenario();
  if (!scenario) return;

  placeCoord("start", scenario.start_lon, scenario.start_lat, () => {
    placeCoord("end", scenario.end_lon, scenario.end_lat);
  });
}

function fetchBenchConfig() {
  return fetchJson("/api/bench/config")
    .then((config) => {
      state.benchConfig = config;

      const defaults = config.defaults || {};
      for (const [key, value] of Object.entries(defaults)) {
        if (key === "algo") state.benchSelection.algo = normalizeBenchAlgoId(value);
        else state.benchSelection[key] = value;
      }

      const firstScenario = (config.scenarios || [])[0];
      if (firstScenario) state.benchSelection.scenario = firstScenario.name;

      applyBenchOptionCoupling("algo");
      applyBenchOptionCoupling("graph_storage");

      renderModeControls();
      renderModeDependentUi();
      renderBenchControls();

      applyBenchScenario();
      refreshSelectedMode();
    })
    .catch((e) => {
      setStatus("Erreur /api/bench/config : " + e.message);
      const status = byId("bench-status");
      if (status) status.textContent = "Erreur chargement bench: " + e.message;
    });
}

export {
  benchAlgorithmDescriptor,
  benchOptionLabel,
  bucketVariantKey,
  currentScenario,
  fetchBenchConfig,
  hasStateOptions,
  isBucketAlgo,
  isBucketDecoupled,
  isCountdownAlgo,
  normalizeBenchAlgoId,
  renderVisualizeLayoutControls,
  toggleVisualizeSide,
  usesDecoupledState,
};
