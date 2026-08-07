/** Popup window controller: bridges the Rust hook to the radial selector. */

import { Radial } from "./radial.js";
import { applyTheme, watchSystemTheme } from "./theme.js";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

/** Safety net: never leave a stale popup on screen if an event is lost. */
const MAX_OPEN_MS = 15000;

const radial = new Radial(document);
let config = null;
let autoCloseTimer = null;

radial
  .on("hover", (index) => invoke("popup_hover", { index }))
  .on("choose", (index) => invoke("popup_choose", { index }))
  .on("cancel", () => invoke("popup_cancel"));

function armAutoClose() {
  clearTimeout(autoCloseTimer);
  autoCloseTimer = setTimeout(() => invoke("popup_cancel"), MAX_OPEN_MS);
}

listen("torch://show", ({ payload }) => {
  radial.show(payload);
  armAutoClose();
});

listen("torch://selection", ({ payload }) => radial.select(payload.index));

listen("torch://hide", () => {
  clearTimeout(autoCloseTimer);
  radial.hide();
});

listen("torch://config", ({ payload }) => {
  config = payload;
  applyTheme(config);
});

window.addEventListener("blur", () => {
  if (radial.open) invoke("popup_cancel");
});

watchSystemTheme(() => config);

invoke("get_config").then((loaded) => {
  config = loaded;
  applyTheme(config);
});
