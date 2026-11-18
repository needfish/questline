const escapeHtml = (text) =>
  text.replace(/[&<>"']/g, (char) => `&#${char.charCodeAt(0)};`);

const inline = (text) => text
  .replace(/`([^`]+)`/g, "<code>$1</code>")
  .replace(/\*\*([^*]+)\*\*/g, "<b>$1</b>")
  .replace(/\*([^*]+)\*/g, "<i>$1</i>")
  .replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2" target="_blank" rel="noreferrer">$1</a>');

const markdown = (source) => source.split("\n").map((raw) => {
  const esc = (text) => inline(escapeHtml(text));
  if (raw.startsWith("### ")) return `<h3>${esc(raw.slice(4))}</h3>`;
  if (raw.startsWith("## ")) return `<h2>${esc(raw.slice(3))}</h2>`;
  if (raw.startsWith("# ")) return `<h1>${esc(raw.slice(2))}</h1>`;
  if (raw.startsWith("- ")) return `<li>${esc(raw.slice(2))}</li>`;
  if (raw.startsWith("> ")) return `<blockquote>${esc(raw.slice(2))}</blockquote>`;
  if (!raw) return "<br>";
  return `<p>${esc(raw)}</p>`;
}).join("");

const state = {
  lines: [],
  view: "all",
  project: null,
  selected: null,
  write: true,
  collapsed: new Set(),
};

async function api(path, options) {
  let res;
  try {
    res = await fetch(path, options);
  } catch {
    throw new Error("server unreachable");
  }
  const text = await res.text();
  if (!res.ok) {
    let reason = `server ${res.status}`;
    try {
      const parsed = JSON.parse(text);
      if (typeof parsed === "string") reason = parsed;
    } catch {}
    throw new Error(reason);
  }
  const type = res.headers.get("content-type") ?? "";
  try {
    if (!type.includes("json")) throw new Error();
    return JSON.parse(text);
  } catch {
    throw new Error(`${path} bad response`);
  }
}

function tree(lines) {
  const children = new Map();
  for (const line of lines) {
    const key = line.parent ?? "";
    if (!children.has(key)) children.set(key, []);
    children.get(key).push(line);
  }
  return children;
}

function rows() {
  const children = tree(state.lines);
  const scope = state.project ? subtreeIds(state.project) : null;
  const out = [];
  const walk = (key, depth) => {
    for (const line of children.get(key) ?? []) {
      const hasChildren = (children.get(line.id) ?? []).length > 0;
      const isCollapsed = state.collapsed.has(line.id);
      out.push({ line, depth, hasChildren, isCollapsed });
      if (!isCollapsed) walk(line.id, depth + 1);
    }
  };
  walk("", 0);
  return out.filter((row) => (!scope || scope.has(row.line.id)) && matches(row.line));
}

function subtreeIds(rootId) {
  const children = tree(state.lines);
  const ids = new Set();
  const walk = (key) => {
    ids.add(key);
    for (const line of children.get(key) ?? []) walk(line.id);
  };
  walk(rootId);
  return ids;
}

const areas = () => state.lines.filter((line) => line.parent === null && line.kind === "Open");

function openCount(rootId) {
  const scope = subtreeIds(rootId);
  return state.lines.filter((line) => scope.has(line.id) && line.status !== "Done").length;
}

function matches(line) {
  if (state.view === "open") return line.kind === "Open";
  if (state.view === "closed") return line.kind !== "Open";
  return true;
}

function count(view) {
  return state.lines.filter((line) => view === "all" || (view === "open") === (line.kind === "Open")).length;
}

async function refresh() {
  try {
    state.lines = await api("/items");
    render();
  } catch (error) {
    showError(error.message);
  }
}

function showError(text) {
  const el = document.getElementById("error");
  el.textContent = text;
  el.hidden = !text;
}

async function send(path, body) {
  try {
    await api(path, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
    showError("");
    await refresh();
  } catch (error) {
    showError(error.message);
  }
}

const toggle = (id) => {
  state.collapsed.has(id) ? state.collapsed.delete(id) : state.collapsed.add(id);
  render();
};

const move = (id, body) => send(`/items/${id}/move`, body);

const done = (id) => send(`/items/${id}`, {});

const retime = (id, deadline) => send(`/items/${id}/deadline`, deadline);

const renote = (id, notes) => send(`/items/${id}/notes`, notes);

const create = (areaId, title, deadline) =>
  send("/items", { title, parent: areaId, open: areaId === null, deadline });

function render() {
  renderViews();
  renderProjects();
  renderOutline();
  renderWorkspace();
}

function renderWorkspace() {
  const host = document.getElementById("workspace");
  if (!state.selected) {
    host.replaceChildren();
    host.hidden = true;
    return;
  }
  const line = state.lines.find((item) => item.id === state.selected);
  if (!line) {
    state.selected = null;
    host.hidden = true;
    return;
  }
  host.hidden = false;
  const head = document.createElement("h2");
  head.textContent = line.title;
  const facts = document.createElement("p");
  facts.className = "facts";
  facts.textContent = [
    line.kind === "Open" ? "open-ended" : `due ${line.kind.Closed.deadline}`,
    line.status !== "Done" ? "active" : "done",
    line.tags.length ? line.tags.join(", ") : null,
  ].filter(Boolean).join(" · ");
  const label = document.createElement("label");
  label.textContent = "prep notes";
  const notes = document.createElement("textarea");
  notes.value = line.notes;
  notes.addEventListener("change", () => renote(line.id, notes.value));
  const preview = document.createElement("div");
  preview.className = "preview";
  preview.innerHTML = markdown(line.notes);
  const toggle = document.createElement("button");
  toggle.textContent = state.write ? "preview" : "write";
  toggle.addEventListener("click", () => {
    state.write = !state.write;
    render();
  });
  label.append(toggle);
  host.replaceChildren(head, facts, label, state.write ? notes : preview);
}

function renderProjects() {
  const input = document.getElementById("title");
  input.placeholder = state.project === null ? "new area" : "new project in area";
  document.getElementById("deadline").hidden = state.project === null;
  const nav = document.getElementById("projects");
  const buttons = [{ id: null, title: "Areas", badge: state.lines.filter((line) => line.status !== "Done").length }]
    .concat(areas().map((line) => ({ id: line.id, title: line.title, badge: openCount(line.id) })));
  nav.replaceChildren(...buttons.map((project) => {
    const button = document.createElement("button");
    if (project.id === state.project) button.classList.add("active");
    const label = document.createElement("span");
    label.className = "label";
    label.textContent = project.title;
    const badge = document.createElement("span");
    badge.textContent = project.badge;
    button.append(label, badge);
    button.addEventListener("click", () => {
      state.project = project.id;
      render();
    });
    return button;
  }));
}

function renderViews() {
  const nav = document.getElementById("views");
  nav.replaceChildren(...["all", "open", "closed"].map((view) => {
    const button = document.createElement("button");
    if (view === state.view) button.classList.add("active");
    const label = document.createElement("span");
    label.className = "label";
    label.textContent = { now: "Now", all: "All lines", open: "Open", closed: "Closed" }[view];
    const badge = document.createElement("span");
    badge.textContent = count(view);
    button.append(label, badge);
    button.addEventListener("click", () => {
      state.view = view;
      render();
    });
    return button;
  }));
}

function renderOutline() {
  const host = document.getElementById("outline");
  host.replaceChildren(...rows().map((row) => rowElement(row)));
}

function rowElement(row) {
  const el = document.createElement("div");
  el.className = `row${row.line.status === "Done" ? " done" : ""}${row.depth > 0 ? " child-guide" : ""}`;
  el.style.marginLeft = `calc(${row.depth} * var(--indent))`;
  el.draggable = true;
  el.addEventListener("dragstart", (event) => {
    event.dataTransfer.setData("text/plain", row.line.id);
    event.dataTransfer.effectAllowed = "move";
  });
  el.addEventListener("dragover", (event) => {
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
    el.className = rowClassName(row, dropZone(el, event));
  });
  el.addEventListener("dragleave", () => {
    el.className = rowClassName(row, null);
  });
  el.addEventListener("drop", (event) => {
    event.preventDefault();
    const zone = dropZone(el, event);
    el.className = rowClassName(row, null);
    move(event.dataTransfer.getData("text/plain"), dropInstruction(row, zone));
  });
  el.append(
    row.hasChildren ? toggleElement(row) : spacer(),
    doneButton(row.line),
    titleElement(row.line),
    kindBadge(row.line),
  );
  return el;
}

const rowClassName = (row, zone) =>
  `row${row.line.status === "Done" ? " done" : ""}${row.depth > 0 ? " child-guide" : ""}`
  + (zone === "child" ? " drop-into" : zone === "before" ? " drop-before" : zone === "after" ? " drop-after" : "");

function dropZone(el, event) {
  const rect = el.getBoundingClientRect();
  const fraction = (event.clientY - rect.top) / rect.height;
  if (fraction < 0.3) return "before";
  if (fraction > 0.7) return "after";
  return "child";
}

function dropInstruction(row, zone) {
  const children = tree(state.lines).get(row.line.parent ?? "") ?? [];
  const index = children.indexOf(row.line);
  if (zone === "child") {
    const firstChild = tree(state.lines).get(row.line.id)?.[0];
    return { parent: row.line.id, before: firstChild?.id ?? null };
  }
  if (zone === "before") {
    return { parent: row.line.parent ?? null, before: row.line.id };
  }
  return { parent: row.line.parent ?? null, before: children[index + 1]?.id ?? null };
}

const spacer = () => {
  const el = document.createElement("span");
  el.style.width = "18px";
  return el;
};

function toggleElement(row) {
  const el = document.createElement("button");
  el.className = "toggle";
  el.textContent = row.isCollapsed ? "▸" : "▾";
  el.setAttribute("aria-label", row.isCollapsed ? "expand" : "collapse");
  el.addEventListener("click", () => toggle(row.line.id));
  return el;
}

function doneButton(line) {
  const el = document.createElement("button");
  el.textContent = line.status === "Done" ? "✓" : "○";
  el.addEventListener("click", () => done(line.id));
  return el;
}

const titleElement = (line) => {
  const el = document.createElement("span");
  el.className = "title";
  el.textContent = line.title;
  el.title = line.id;
  el.addEventListener("click", () => {
    state.selected = state.selected === line.id ? null : line.id;
    render();
  });
  return el;
};

function kindBadge(line) {
  if (line.kind === "Open") {
    const el = document.createElement("span");
    el.className = "badge";
    el.textContent = "open";
    return el;
  }
  const el = document.createElement("button");
  el.className = "badge closed";
  el.textContent = `due ${line.kind.Closed.deadline}`;
  el.title = "click to change deadline";
  el.addEventListener("click", () => {
    const input = document.createElement("input");
    input.type = "date";
    input.value = line.kind.Closed.deadline;
    input.addEventListener("change", () => {
      if (input.value) retime(line.id, input.value);
    });
    input.addEventListener("keydown", (event) => {
      if (event.key === "Escape") render();
    });
    el.replaceChildren(input);
    input.focus();
    input.showPicker?.();
  });
  return el;
}

const addOpen = document.getElementById("add-open");
const addForm = document.getElementById("add");
addOpen.addEventListener("click", () => {
  addOpen.hidden = true;
  addForm.hidden = false;
  document.getElementById("title").focus();
});
document.getElementById("add-close").addEventListener("click", () => {
  addForm.hidden = true;
  addOpen.hidden = false;
});

addForm.addEventListener("submit", (event) => {
  event.preventDefault();
  const input = document.getElementById("title");
  const deadlineInput = document.getElementById("deadline");
  const title = input.value.trim();
  if (!title) return;
  input.value = "";
  deadlineInput.value = "";
  addForm.hidden = true;
  addOpen.hidden = false;
  create(state.project, title, deadlineInput.value || null);
});

refresh();
