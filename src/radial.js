/**
 * Radial selector rendering and pointer interaction.
 *
 * Selection state is owned by the Rust backend; this module only renders what
 * it is told and reports pointer intent back through callbacks. Every visual
 * property is interpolated in a single rAF loop so hover, magnetic pull and the
 * accent bloom all move together instead of fighting each other.
 */

const RADIUS = 132;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;
const MAGNET_RANGE = 130;
const MAGNET_STRENGTH = 9;

const PRETTY_KEYS = {
  enter: "Enter",
  return: "Enter",
  space: "Space",
  tab: "Tab",
  esc: "Esc",
  escape: "Esc",
  backspace: "Bksp",
  delete: "Del",
  del: "Del",
  insert: "Ins",
  pageup: "PgUp",
  pagedown: "PgDn",
  home: "Home",
  end: "End",
  up: "\u2191",
  down: "\u2193",
  left: "\u2190",
  right: "\u2192",
  ctrl: "Ctrl",
  control: "Ctrl",
  shift: "Shift",
  alt: "Alt",
  win: "Win",
};

export function prettyKey(name) {
  const raw = String(name ?? "").trim();
  if (!raw) return "";
  if (raw.includes("+")) {
    return raw
      .split("+")
      .map((part) => prettyKey(part))
      .join("+");
  }
  const lower = raw.toLowerCase();
  if (PRETTY_KEYS[lower]) return PRETTY_KEYS[lower];
  if (lower.length === 1) return raw.toUpperCase();
  if (/^f\d{1,2}$/.test(lower)) return lower.toUpperCase();
  return raw.charAt(0).toUpperCase() + raw.slice(1);
}

const lerp = (a, b, t) => a + (b - a) * t;

/** Shortest signed distance between two angles in degrees. */
function angleDelta(from, to) {
  return ((((to - from) % 360) + 540) % 360) - 180;
}

export class Radial {
  constructor(root) {
    this.stage = root.querySelector("#stage");
    this.glass = root.querySelector("#glass");
    this.itemsHost = root.querySelector("#items");
    this.coreKey = root.querySelector("#core-key");
    this.coreHint = root.querySelector("#core-hint");
    this.needle = root.querySelector("#needle");
    this.progress = root.querySelector("#orbit-progress");

    this.items = [];
    this.nodes = [];
    this.index = 0;
    this.open = false;
    this.pointer = null;
    this.needleAngle = -90;
    this.handlers = { hover: () => {}, choose: () => {}, cancel: () => {} };

    this.progress.style.strokeDasharray = `0 ${CIRCUMFERENCE}`;

    this.glass.addEventListener("pointermove", (event) => this.#onPointerMove(event));
    this.glass.addEventListener("pointerleave", () => {
      this.pointer = null;
    });
    this.glass.addEventListener("pointerdown", (event) => this.#onPointerDown(event));
    root.addEventListener("contextmenu", (event) => event.preventDefault());

    this.#loop();
  }

  on(event, handler) {
    this.handlers[event] = handler;
    return this;
  }

  show({ trigger, items, index = 0 }) {
    this.items = Array.isArray(items) ? items : [];
    this.index = Math.min(Math.max(index, 0), Math.max(this.items.length - 1, 0));
    this.coreKey.textContent = prettyKey(trigger);
    this.coreKey.classList.toggle("small", prettyKey(trigger).length > 3);
    this.coreHint.textContent = this.items.length ? "release to pick" : "no mapping";
    this.#render();
    this.needleAngle = this.#angleFor(this.index);
    this.open = true;
    this.stage.classList.remove("leaving");
    // Force a reflow so the entrance transition always replays.
    void this.stage.offsetWidth;
    this.stage.classList.add("visible");
    this.stage.setAttribute("aria-hidden", "false");
  }

  select(index) {
    if (index === this.index || index < 0 || index >= this.items.length) return;
    this.index = index;
    this.#syncActiveClass();
  }

  hide() {
    if (!this.open) return;
    this.open = false;
    this.pointer = null;
    this.stage.classList.remove("visible");
    this.stage.classList.add("leaving");
    this.stage.setAttribute("aria-hidden", "true");
  }

  #angleFor(index) {
    const count = this.items.length || 1;
    return -90 + (360 / count) * index;
  }

  #render() {
    this.itemsHost.textContent = "";
    this.nodes = this.items.map((name, i) => {
      const node = document.createElement("div");
      const label = prettyKey(name);
      node.className = "item";
      if (label.length > 2) node.classList.add("small");
      node.dataset.index = String(i);
      node.style.animationDelay = `${i * 26}ms`;
      const span = document.createElement("span");
      span.className = "label";
      span.textContent = label;
      node.append(span);
      this.itemsHost.append(node);
      return { node, angle: this.#angleFor(i), scale: 1, pull: 0, x: 0, y: 0 };
    });
    this.#syncActiveClass();
  }

  #syncActiveClass() {
    this.nodes.forEach((item, i) => item.node.classList.toggle("active", i === this.index));
  }

  #onPointerMove(event) {
    const rect = this.glass.getBoundingClientRect();
    const x = event.clientX - rect.left - rect.width / 2;
    const y = event.clientY - rect.top - rect.height / 2;
    this.pointer = { x, y };

    // Magnetic snapping: anything outside the dead zone commits to the nearest
    // item by angle, so the user never has to land precisely on a target.
    if (!this.items.length || Math.hypot(x, y) < 46) return;
    const angle = (Math.atan2(y, x) * 180) / Math.PI;
    let best = 0;
    let bestDelta = Infinity;
    this.nodes.forEach((item, i) => {
      const delta = Math.abs(angleDelta(item.angle, angle));
      if (delta < bestDelta) {
        bestDelta = delta;
        best = i;
      }
    });
    if (best !== this.index) {
      this.select(best);
      this.handlers.hover(best);
    }
  }

  #onPointerDown(event) {
    if (!this.open) return;
    const target = event.target.closest(".item");
    const index = target ? Number(target.dataset.index) : this.index;
    if (!this.items.length) {
      this.handlers.cancel();
      return;
    }
    const node = this.nodes[index]?.node;
    if (node) node.classList.add("pressed");
    this.handlers.choose(index);
  }

  #loop() {
    const frame = () => {
      this.#interpolate();
      requestAnimationFrame(frame);
    };
    requestAnimationFrame(frame);
  }

  #interpolate() {
    if (!this.nodes.length) return;

    const targetAngle = this.needleAngle + angleDelta(this.needleAngle, this.#angleFor(this.index));
    this.needleAngle = lerp(this.needleAngle, targetAngle, 0.24);

    for (let i = 0; i < this.nodes.length; i++) {
      const item = this.nodes[i];
      const active = i === this.index;
      const rad = (item.angle * Math.PI) / 180;
      const baseX = Math.cos(rad) * RADIUS;
      const baseY = Math.sin(rad) * RADIUS;

      let pull = 0;
      if (this.pointer) {
        const distance = Math.hypot(this.pointer.x - baseX, this.pointer.y - baseY);
        pull = Math.max(0, 1 - distance / MAGNET_RANGE) * MAGNET_STRENGTH;
      }
      item.pull = lerp(item.pull, pull, 0.18);
      item.scale = lerp(item.scale, active ? 1.1 : 1, 0.2);

      const radius = RADIUS + item.pull;
      item.node.style.transform =
        `rotate(${item.angle}deg) translate(${radius}px) ` +
        `rotate(${-item.angle}deg) scale(${item.scale.toFixed(3)})`;
    }

    this.needle.style.transform = `rotate(${this.needleAngle.toFixed(2)}deg)`;

    const segment = Math.min(CIRCUMFERENCE / this.nodes.length, 150) * 0.55;
    const fraction = (((this.needleAngle + 90) % 360) + 360) % 360 / 360;
    this.progress.style.strokeDasharray = `${segment} ${CIRCUMFERENCE - segment}`;
    this.progress.style.strokeDashoffset = `${-(fraction * CIRCUMFERENCE) + segment / 2}`;

    const rad = (this.needleAngle * Math.PI) / 180;
    this.glass.style.setProperty("--glow-x", `${50 + (Math.cos(rad) * RADIUS * 100) / 384}%`);
    this.glass.style.setProperty("--glow-y", `${50 + (Math.sin(rad) * RADIUS * 100) / 384}%`);
  }
}
