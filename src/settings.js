/** Settings window: edits config.json through the Rust backend. */

import { Radial, prettyKey } from "./radial.js";
import { applyTheme, watchSystemTheme } from "./theme.js";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWindow } = window.__TAURI__.window;

const $ = (id) => document.getElementById(id);

const preview = new Radial(document);
let config = null;
let saveTimer = null;
let capture = null;

// --- key capture ------------------------------------------------------

const CODE_ALIASES = {
  Escape: "esc",
  Enter: "enter",
  NumpadEnter: "enter",
  Space: "space",
  Tab: "tab",
  Backspace: "backspace",
  Delete: "delete",
  Insert: "insert",
  Home: "home",
  End: "end",
  PageUp: "pageup",
  PageDown: "pagedown",
  ArrowUp: "up",
  ArrowDown: "down",
  ArrowLeft: "left",
  ArrowRight: "right",
  CapsLock: "capslock",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Backquote: "`",
  Comma: ",",
  Period: ".",
  Slash: "/",
  NumpadAdd: "add",
  NumpadSubtract: "subtract",
  NumpadMultiply: "multiply",
  NumpadDivide: "divide",
  NumpadDecimal: "decimal",
};

/** Layout independent key name that matches the Rust `vk_from_name` table. */
function keyFromEvent(event) {
  const code = event.code;
  if (/^Key[A-Z]$/.test(code)) return code.slice(3).toLowerCase();
  if (/^Digit\d$/.test(code)) return code.slice(5);
  if (/^Numpad\d$/.test(code)) return `numpad${code.slice(6)}`;
  if (/^F\d{1,2}$/.test(code)) return code.toLowerCase();
  if (CODE_ALIASES[code]) return CODE_ALIASES[code];
  if (event.key && event.key.length === 1) return event.key.toLowerCase();
  return null;
}

function captureKey(subtitle) {
  $("capture-sub").textContent = subtitle;
  $("capture").hidden = false;
  return new Promise((resolve) => {
    capture = (value) => {
      capture = null;
      $("capture").hidden = true;
      resolve(value);
    };
  });
}

window.addEventListener(
  "keydown",
  (event) => {
    if (!capture) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.code === "Escape") return capture(null);
    const key = keyFromEvent(event);
    if (key) capture(key);
  },
  true
);

$("capture-cancel").addEventListener("click", () => capture?.(null));

// --- persistence ------------------------------------------------------

function save({ rerender = false } = {}) {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(async () => {
    const saved = await invoke("set_config", { config });
    config = saved;
    if (rerender) renderMappings();
    renderPreview();
  }, 90);
}

function patch(partial, options) {
  config = { ...config, ...partial };
  applyTheme(config);
  save(options);
}

// --- rendering --------------------------------------------------------

function renderGeneral() {
  $("enabled").checked = config.enabled;
  $("startup").checked = config.startup;
  $("hold-time").value = config.hold_time;
  $("hold-time-value").textContent = `${Number(config.hold_time).toFixed(2)}s`;
  $("instant").checked = config.instant_passthrough;
  $("blur").value = config.custom_theme.blur;
  $("blur-value").textContent = `${config.custom_theme.blur}px`;
  $("accent").value = config.custom_theme.accent;
  $("bg").value = config.custom_theme.bg;
  $("text").value = config.custom_theme.text;
  $("highlight").value = config.custom_theme.highlight;

  const pill = $("status-pill");
  pill.textContent = config.enabled ? "Active" : "Paused";
  pill.classList.toggle("off", !config.enabled);

  setSegment("theme", config.theme);
  setSegment("popup-anchor", config.popup_anchor);
  document
    .querySelectorAll("[data-custom-only]")
    .forEach((node) => node.classList.toggle("disabled", config.theme !== "custom"));
}

function setSegment(id, value) {
  $(id)
    .querySelectorAll("button")
    .forEach((button) => button.classList.toggle("active", button.dataset.value === value));
}

function renderMappings() {
  const host = $("mappings");
  host.textContent = "";
  const entries = Object.entries(config.mapping);
  if (!entries.length) {
    const empty = document.createElement("p");
    empty.className = "empty";
    empty.textContent = "No mappings yet. Add a trigger to get started.";
    host.append(empty);
    return;
  }

  entries.forEach(([trigger, targets], row) => {
    const mapping = document.createElement("div");
    mapping.className = "mapping";
    mapping.style.animationDelay = `${row * 30}ms`;

    const keycap = document.createElement("button");
    keycap.className = "keycap";
    keycap.textContent = prettyKey(trigger);
    keycap.title = "Change trigger key";
    keycap.addEventListener("click", async () => {
      const next = await captureKey("It replaces the trigger key");
      if (!next || next === trigger) return;
      const updated = {};
      for (const [key, value] of Object.entries(config.mapping)) {
        updated[key === trigger ? next : key] = value;
      }
      patch({ mapping: updated }, { rerender: true });
      renderMappings();
    });

    const arrow = document.createElement("span");
    arrow.className = "arrow";
    arrow.textContent = "\u2192";

    const list = document.createElement("div");
    list.className = "targets";
    targets.forEach((target, index) => {
      const chip = document.createElement("span");
      chip.className = "chip";
      chip.append(document.createTextNode(prettyKey(target)));
      const remove = document.createElement("button");
      remove.textContent = "\u00d7";
      remove.title = "Remove";
      remove.addEventListener("click", () => {
        const next = targets.filter((_, i) => i !== index);
        const mappingCopy = { ...config.mapping };
        if (next.length) mappingCopy[trigger] = next;
        else delete mappingCopy[trigger];
        patch({ mapping: mappingCopy }, { rerender: true });
        renderMappings();
      });
      chip.append(remove);
      list.append(chip);
    });

    const add = document.createElement("button");
    add.className = "chip add";
    add.textContent = "+ key";
    add.addEventListener("click", async () => {
      const key = await captureKey(`It is added to the ${prettyKey(trigger)} wheel`);
      if (!key || targets.includes(key)) return;
      patch(
        { mapping: { ...config.mapping, [trigger]: [...targets, key] } },
        { rerender: true }
      );
      renderMappings();
    });
    list.append(add);

    const remove = document.createElement("button");
    remove.className = "ghost-button remove";
    remove.textContent = "Delete";
    remove.addEventListener("click", () => {
      const mappingCopy = { ...config.mapping };
      delete mappingCopy[trigger];
      patch({ mapping: mappingCopy }, { rerender: true });
      renderMappings();
    });

    mapping.append(keycap, arrow, list, remove);
    host.append(mapping);
  });
}

function renderPreview() {
  const [trigger, items] = Object.entries(config.mapping)[0] ?? ["s", ["w", "x", "e"]];
  preview.show({ trigger, items, index: 0 });
}

function renderAll() {
  applyTheme(config);
  renderGeneral();
  renderMappings();
  renderPreview();
}

// --- wiring -----------------------------------------------------------

$("enabled").addEventListener("change", (e) => patch({ enabled: e.target.checked }));
$("startup").addEventListener("change", (e) => patch({ startup: e.target.checked }));
$("instant").addEventListener("change", (e) => patch({ instant_passthrough: e.target.checked }));

$("hold-time").addEventListener("input", (e) => {
  const value = Number(e.target.value);
  $("hold-time-value").textContent = `${value.toFixed(2)}s`;
  patch({ hold_time: value });
});

$("blur").addEventListener("input", (e) => {
  const blur = Number(e.target.value);
  $("blur-value").textContent = `${blur}px`;
  patch({ custom_theme: { ...config.custom_theme, blur } });
});

for (const id of ["accent", "bg", "text", "highlight"]) {
  $(id).addEventListener("input", (e) =>
    patch({ custom_theme: { ...config.custom_theme, [id]: e.target.value } })
  );
}

$("theme").addEventListener("click", (event) => {
  const button = event.target.closest("button");
  if (!button) return;
  patch({ theme: button.dataset.value });
  renderGeneral();
});

$("popup-anchor").addEventListener("click", (event) => {
  const button = event.target.closest("button");
  if (!button) return;
  patch({ popup_anchor: button.dataset.value });
  renderGeneral();
});

$("add-mapping").addEventListener("click", async () => {
  const trigger = await captureKey("It becomes the trigger key");
  if (!trigger || config.mapping[trigger]) return;
  const target = await captureKey(`First key on the ${prettyKey(trigger)} wheel`);
  if (!target) return;
  patch({ mapping: { ...config.mapping, [trigger]: [target] } }, { rerender: true });
  renderMappings();
});

$("close-window").addEventListener("click", () => getCurrentWindow().close());
$("quit").addEventListener("click", () => invoke("quit"));

listen("torch://config", ({ payload }) => {
  const mappingChanged = JSON.stringify(payload.mapping) !== JSON.stringify(config?.mapping);
  config = payload;
  applyTheme(config);
  renderGeneral();
  if (mappingChanged) renderMappings();
  renderPreview();
});

watchSystemTheme(() => config);

invoke("get_config").then((loaded) => {
  config = loaded;
  renderAll();
});
