const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWindow } = window.__TAURI__.window;
const $ = (id) => document.getElementById(id);

const MODIFIERS = [
  { id: "ctrl", label: "Ctrl" },
  { id: "alt", label: "Alt" },
  { id: "shift", label: "Shift" },
  { id: "win", label: "Win" },
];
const KEY_ROWS = [
  ["esc", ...Array.from({ length: 12 }, (_, i) => `f${i + 1}`)],
  ["`", ..."1234567890-=".split(""), "backspace"],
  ["tab", ..."qwertyuiop[]\\".split("")],
  ["capslock", ..."asdfghjkl;'".split(""), "enter"],
  ["shift", ..."zxcvbnm,./".split(""), "up", "shift"],
  ["ctrl", "win", "alt", "space", "alt", "win", "menu", "left", "down", "right"],
  ["insert", "home", "pageup", "delete", "end", "pagedown", "numlock", "divide", "multiply", "subtract"],
  ["numpad7", "numpad8", "numpad9", "add", "numpad4", "numpad5", "numpad6", "numpad1", "numpad2", "numpad3", "numpad0", "decimal"],
];
const DISPLAY = { esc: "Esc", backspace: "⌫", tab: "Tab", capslock: "Caps", enter: "Enter", shift: "Shift", ctrl: "Ctrl", alt: "Alt", win: "Win", menu: "Menu", space: "Space", up: "↑", down: "↓", left: "←", right: "→", insert: "Ins", home: "Home", pageup: "PgUp", delete: "Del", end: "End", pagedown: "PgDn", numlock: "Num", divide: "÷", multiply: "×", subtract: "−", add: "+", decimal: ".", "`": "`" };
let config;
let selectedModifiers = new Set(["ctrl", "alt"]);
let sourceKey = null;
let targetKey = null;
let keyboardChoice = "source";
let saveChain = Promise.resolve();
let saveTimer;
let configRevision = 0;
let savedRevision = 0;

function label(key) { return DISPLAY[key] ?? key.toUpperCase(); }
function renderModifiers() {
  const host = $("modifiers");
  host.textContent = "";
  MODIFIERS.forEach(({ id, label: name }) => {
    const button = document.createElement("button");
    button.className = `modifier-button${selectedModifiers.has(id) ? " selected" : ""}`;
    button.type = "button";
    button.textContent = name;
    button.setAttribute("aria-pressed", String(selectedModifiers.has(id)));
    button.addEventListener("click", () => {
      selectedModifiers.has(id) ? selectedModifiers.delete(id) : selectedModifiers.add(id);
      renderModifiers();
      renderBuilder();
    });
    host.append(button);
  });
}

function renderKeyboard() {
  const host = $("keyboard");
  host.textContent = "";
  KEY_ROWS.forEach((row) => {
    const line = document.createElement("div");
    line.className = "keyboard-row";
    row.forEach((key) => {
      const button = document.createElement("button");
      button.type = "button";
      button.className = `keyboard-key${key === "space" ? " space-key" : ""}${key === sourceKey ? " source-key" : ""}${key === targetKey ? " target-key" : ""}`;
      button.textContent = label(key);
      button.title = `Select ${label(key)} as ${keyboardChoice === "source" ? "working key" : "replacement key"}`;
      button.addEventListener("click", () => {
        if (["ctrl", "alt", "shift", "win"].includes(key)) {
          const name = key === "win" ? "win" : key;
          selectedModifiers.has(name) ? selectedModifiers.delete(name) : selectedModifiers.add(name);
          renderModifiers();
        } else if (keyboardChoice === "source") {
          sourceKey = key;
          keyboardChoice = "target";
        } else {
          targetKey = key;
          keyboardChoice = "source";
        }
        renderKeyboard();
        renderBuilder();
      });
      line.append(button);
    });
    host.append(line);
  });
}

function renderBuilder() {
  $("source-picked").textContent = sourceKey ? label(sourceKey) : "Choose a key below";
  $("target-picked").textContent = targetKey ? label(targetKey) : "Choose a key below";
  $("keyboard-mode").textContent = keyboardChoice === "source" ? "Select the working key" : "Now select the replacement key";
  $("add-remap").disabled = !sourceKey || !targetKey || selectedModifiers.size === 0;
}

function renderRemaps() {
  const host = $("remaps");
  const entries = Object.entries(config.remaps ?? {});
  host.textContent = "";
  $("remap-count").textContent = String(entries.length);
  if (!entries.length) {
    const empty = document.createElement("p");
    empty.className = "empty-remaps";
    empty.textContent = "No remaps yet. Build one above to rescue a key.";
    host.append(empty);
    return;
  }
  entries.forEach(([source, target]) => {
    const row = document.createElement("div");
    row.className = "remap-row";
    const from = document.createElement("div");
    from.className = "remap-source";
    source.split("+").forEach((key) => { const cap = document.createElement("kbd"); cap.textContent = label(key); from.append(cap); });
    const arrow = document.createElement("span"); arrow.className = "remap-arrow"; arrow.textContent = "→";
    const to = document.createElement("kbd"); to.className = "remap-target"; to.textContent = label(target);
    const remove = document.createElement("button"); remove.className = "remove-remap"; remove.type = "button"; remove.textContent = "Remove";
    remove.addEventListener("click", () => { const remaps = { ...config.remaps }; delete remaps[source]; patch({ remaps }); });
    row.append(from, arrow, to, remove);
    host.append(row);
  });
}

function renderStatus() {
  const pill = $("status-pill");
  pill.textContent = config.enabled ? "Active" : "Paused";
  pill.classList.toggle("off", !config.enabled);
  $("enabled").checked = config.enabled;
  $("startup").checked = config.startup;
}

function persist() {
  clearTimeout(saveTimer);
  const revision = ++configRevision;
  $("save-status").textContent = "Saving…";
  saveTimer = setTimeout(() => {
    const snapshot = structuredClone(config);
    saveChain = saveChain.then(() => invoke("set_config", { config: snapshot })).then((saved) => {
      if (revision === configRevision) {
        config = saved;
        savedRevision = revision;
        $("save-status").textContent = "Saved to this device";
        renderStatus();
      }
    }).catch((error) => { $("save-status").textContent = `Could not save: ${error}`; });
  }, 100);
}

function patch(partial) {
  config = { ...config, ...partial };
  renderRemaps();
  renderStatus();
  persist();
}

$("add-remap").addEventListener("click", () => {
  const modifiers = MODIFIERS.filter(({ id }) => selectedModifiers.has(id)).map(({ id }) => id);
  const source = [...modifiers, sourceKey].join("+");
  if (config.remaps[source]) {
    $("save-status").textContent = "That shortcut already has a remap";
    return;
  }
  patch({ remaps: { ...(config.remaps ?? {}), [source]: targetKey } });
  sourceKey = null;
  targetKey = null;
  keyboardChoice = "source";
  renderKeyboard();
  renderBuilder();
});

$("enabled").addEventListener("change", (event) => patch({ enabled: event.target.checked }));
$("startup").addEventListener("change", (event) => patch({ startup: event.target.checked }));
$("close-window").addEventListener("click", () => getCurrentWindow().close());
$("quit").addEventListener("click", () => invoke("quit"));

listen("torch://config", ({ payload }) => {
  if (configRevision !== savedRevision) return;
  config = payload;
  renderStatus();
  renderRemaps();
});

renderModifiers();
renderKeyboard();
invoke("get_config").then((loaded) => {
  config = loaded;
  config.remaps ??= {};
  renderStatus();
  renderRemaps();
});

