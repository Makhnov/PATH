(function () {
    "use strict";

    const DEFAULT_CHANNELS = {
        boot: true,
        http: true,
        snap: true,
        status: true,
        render: false,
    };

    const state = {
        channels: { ...DEFAULT_CHANNELS },
        lastStatus: null,
        lastRenderLogs: new Map(),
    };

    function nowMs() {
        return typeof performance !== "undefined" && performance.now
            ? performance.now()
            : Date.now();
    }

    function isEnabled(channel) {
        return state.channels[channel] !== false;
    }

    function setChannel(channel, enabled) {
        state.channels[channel] = Boolean(enabled);
    }

    function enable(channel) {
        setChannel(channel, true);
    }

    function disable(channel) {
        setChannel(channel, false);
    }

    function stringify(value) {
        if (typeof value === "string") return value;
        if (value == null) return String(value);
        try {
            return JSON.stringify(value);
        } catch (_) {
            return String(value);
        }
    }

    function round(value, digits = 3) {
        if (typeof value !== "number" || !Number.isFinite(value)) return value;
        const factor = 10 ** digits;
        return Math.round(value * factor) / factor;
    }

    function compactNode(node) {
        if (!node) return null;
        return {
            label: node.label,
            role: node.role,
            tile: node.tile,
            index: node.index,
            lon: round(node.lon, 7),
            lat: round(node.lat, 7),
            x: round(node.x, 2),
            y: round(node.y, 2),
        };
    }

    function print(level, channel, message, data) {
        if (!isEnabled(channel)) return;
        const prefix = `[viewer:${channel}]`;
        if (data === undefined) console[level](prefix, message);
        else console[level](prefix, message, data);
    }

    function info(channel, message, data) {
        print("log", channel, message, data);
    }

    function warn(channel, message, data) {
        print("warn", channel, message, data);
    }

    function error(channel, message, data) {
        print("error", channel, message, data);
    }

    function debug(...values) {
        info("boot", values.map(stringify).join(" "));
    }

    function status(message, data) {
        const signature = message + "::" + stringify(data);
        if (state.lastStatus === signature) return;
        state.lastStatus = signature;
        info("status", message, data);
    }

    function http(method, url, statusCode, meta) {
        const level = statusCode >= 400 ? "error" : "log";
        print(level, "http", `${method} ${url} -> ${statusCode}`, meta);
    }

    function snapshot(snapshotValue) {
        if (!snapshotValue) {
            warn("boot", "snapshot missing");
            return;
        }
        const segments = snapshotValue.layers?.segments || [];
        const interests = snapshotValue.layers?.interests || [];
        info("boot", "snapshot loaded", {
            segments: segments.length,
            interests: interests.length,
            viewport: snapshotValue.viewport,
        });
    }

    function camera(cameraState) {
        info("boot", "camera reset", {
            centerX: round(cameraState.centerX, 2),
            centerY: round(cameraState.centerY, 2),
            scale: round(cameraState.scale, 6),
            canvas: cameraState.canvas,
        });
    }

    function renderEvery(name, data, intervalMs = 1000) {
        if (!isEnabled("render")) return;
        const now = nowMs();
        const last = state.lastRenderLogs.get(name) || 0;
        if (now - last < intervalMs) return;
        state.lastRenderLogs.set(name, now);
        info("render", name, data);
    }

    function pointPlaced(label, node) {
        info("snap", `${label} placé`, compactNode(node));
    }

    function snapRequest(lon, lat, label, role) {
        info("snap", "request", {
            lon: round(lon, 7),
            lat: round(lat, 7),
            label,
            role,
        });
    }

    function snapResponse(node) {
        info("snap", "response", compactNode(node));
    }

    function snapError(err) {
        error("snap", "failed", { message: err?.message || String(err) });
    }

    window.ViewerLog = {
        enable,
        disable,
        setChannel,
        debug,
        status,
        http,
        snapshot,
        camera,
        renderEvery,
        pointPlaced,
        snapRequest,
        snapResponse,
        snapError,
    };
})();
