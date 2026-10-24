// Démarrage du viewer.

import { log, state } from "./js/state.js";
import { installBenchModalHandlers } from "./js/stats/details.js";
import { installVisualizeHandlers } from "./js/replay/player.js";
import { draw, pan, platform, resetCamera, zoom } from "./js/map.js";
import { byId } from "./js/utils.js";
import { fetchBenchConfig } from "./js/controls.js";

function debugSnapshot(snapshot) {
  log.snapshot(snapshot);
}

function main() {
  installBenchModalHandlers();
  installVisualizeHandlers();

  platform.fetchSnapshot((snapshot) => {
    state.snapshot = snapshot;

    debugSnapshot(snapshot);
    resetCamera();
    const subtitle = byId("subtitle");
    if (subtitle) subtitle.textContent = snapshot.meta.filename;

    platform.installInput({ pan, zoom, redraw: draw });

    fetchBenchConfig().then(() => draw());

    draw();
  });
}

main();
