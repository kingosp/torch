/** Shared theme resolution for the popup and the settings window. */

const DEFAULT_CUSTOM = {
  accent: "#00aaff",
  bg: "#1a1a1a",
  text: "#ffffff",
  highlight: "#ffffff",
  blur: 20,
};

const prefersDark = window.matchMedia("(prefers-color-scheme: dark)");

/** "#1a1a1a" -> "26, 26, 26" so it can drive rgba() with a separate alpha. */
export function hexToRgbTriplet(hex, fallback = "26, 26, 26") {
  const match = /^#?([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(String(hex ?? "").trim());
  if (!match) return fallback;
  let value = match[1];
  if (value.length === 3) {
    value = value
      .split("")
      .map((c) => c + c)
      .join("");
  }
  const int = parseInt(value, 16);
  return `${(int >> 16) & 255}, ${(int >> 8) & 255}, ${int & 255}`;
}

/** Perceived luminance, used to pick readable defaults for custom themes. */
function isLight(hex) {
  const [r, g, b] = hexToRgbTriplet(hex).split(",").map(Number);
  return (0.299 * r + 0.587 * g + 0.114 * b) / 255 > 0.6;
}

export function resolveMode(theme) {
  if (theme === "light" || theme === "dark") return theme;
  if (theme === "custom") return "custom";
  return prefersDark.matches ? "dark" : "light";
}

export function applyTheme(config, root = document.documentElement) {
  const theme = config?.theme ?? "auto";
  const custom = { ...DEFAULT_CUSTOM, ...(config?.custom_theme ?? {}) };
  const mode = resolveMode(theme);

  if (mode === "custom") {
    root.dataset.theme = isLight(custom.bg) ? "light" : "dark";
    root.style.setProperty("--accent", custom.accent);
    root.style.setProperty("--bg", hexToRgbTriplet(custom.bg));
    root.style.setProperty("--text", custom.text);
    root.style.setProperty("--highlight", custom.highlight || custom.text);
    root.style.setProperty("--blur", `${custom.blur}px`);
    return;
  }

  root.dataset.theme = mode;
  root.style.removeProperty("--bg");
  root.style.removeProperty("--text");
  root.style.removeProperty("--highlight");
  root.style.setProperty("--accent", custom.accent);
  root.style.setProperty("--blur", `${custom.blur}px`);
}

/** Re-applies the theme when the OS switches between light and dark. */
export function watchSystemTheme(getConfig) {
  prefersDark.addEventListener("change", () => applyTheme(getConfig()));
}
