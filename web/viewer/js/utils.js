// Outils communs : DOM, nombres, texte et requêtes

import { log } from "./state.js";

function byId(id) {
  return document.getElementById(id);
}

function debug(...values) {
  log.debug(...values);
}

function setStatus(message, meta) {
  log.status(message, meta);
}

function numberField(object, names) {
  if (!object) return null;

  for (const name of names) {
    const value = Number(object[name]);
    if (Number.isFinite(value)) return value;
  }

  return null;
}

function fmt(n, digits = 0) {
  if (n == null || Number.isNaN(Number(n))) return "—";
  return Number(n).toLocaleString("fr-FR", {
    maximumFractionDigits: digits,
  });
}

function fmtBytes(value) {
  if (value == null || !Number.isFinite(Number(value))) return "—";
  const bytes = Number(value);
  if (bytes >= 1024 * 1024) return fmt(bytes / (1024 * 1024), bytes >= 10 * 1024 * 1024 ? 1 : 2) + " Mo";
  if (bytes >= 1024) return fmt(bytes / 1024, bytes >= 100 * 1024 ? 0 : bytes >= 10 * 1024 ? 1 : 2) + " Ko";
  return fmt(bytes, 0) + " O";
}

function formatBytes(value) {
  const bytes = Math.max(0, Number(value || 0));
  if (bytes >= 1024 * 1024) return `${fmt(bytes / (1024 * 1024), 1)} Mo`;
  if (bytes >= 1024) return `${fmt(bytes / 1024, 1)} Ko`;
  return `${fmt(bytes)} o`;
}

function fmtMetric(id, value) {
  if (value == null) return "—";

  if (id.endsWith("_bytes")) {
    return fmtBytes(value);
  }

  if (id.endsWith("_rate") || id.endsWith("_ratio") || id.includes("reduction")) {
    return fmt(value * 100, 1) + " %";
  }

  if (id.endsWith("_ms") || id === "total_ms") {
    return fmt(value, value >= 100 ? 0 : 2) + " ms";
  }

  if (id.endsWith("_us")) {
    return fmt(value, value >= 100 ? 0 : 2) + " µs";
  }

  if (id.includes("per_") || id.startsWith("avg_")) {
    return fmt(value, 2);
  }

  if (id.includes("speedup") || id.includes("ratio_baseline")) {
    return "×" + fmt(value, 2);
  }

  if (id.endsWith("_per_million")) {
    const digits = value >= 100 ? 0 : value >= 10 ? 1 : 2;
    return fmt(value, digits) + " / 1M";
  }

  return fmt(value, 0);
}

function numberOrZero(value) {
  const n = Number(value);
  return Number.isFinite(n) ? n : 0;
}

function clamp(value, min, max) {
  return Math.max(min, Math.min(max, value));
}

function escapeHtml(value) {
  return String(value)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

function fetchJson(url) {
  return fetch(url).then(async (r) => {
    if (!r.ok) {
      const text = await r.text();
      throw new Error(text || r.statusText);
    }
    return r.json();
  });
}

export {
  byId,
  clamp,
  debug,
  escapeHtml,
  fetchJson,
  fmt,
  fmtBytes,
  fmtMetric,
  formatBytes,
  numberField,
  numberOrZero,
  setStatus,
};
