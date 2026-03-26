const { invoke } = window.__TAURI__.core;
const { exists, BaseDirectory, mkdir, writeTextFile, readTextFile, watch , rename, remove, stat, open, create} = window.__TAURI__.fs;
const { join } = window.__TAURI__.path;

const editor = new EasyMDE({element: document.getElementById('right-editor__area')});
let current_editor_file = "";

const searchParams = new URLSearchParams(window.location.search);
const workspace_path = searchParams.get("path");
let last_selected_entry = null;
let last_selected_path = null;
let last_selected_type = null;
let is_loading_entry = null;
let last_tree_snapshot = null;

// --- notification system

const notificationContainer = document.createElement("div");
notificationContainer.id = "notification-container";
document.body.appendChild(notificationContainer);

const NOTIFICATION_TYPES = {
    success: {
        color: "#4caf50",
        icon: `assets/svg/check-circle.svg`
    },
    error: {
        color: "#f44336",
        icon: `assets/svg/x-circle.svg`    
    },
    warning: {
        color: "#ff9800",
        icon: `assets/svg/alert-circle.svg`
    },
    info: {
        color: "#2196f3",
        icon: `assets/svg/info.svg`
    }
};

function notify(message, severity = "info", duration = 1000) {
    const type = NOTIFICATION_TYPES[severity] ?? NOTIFICATION_TYPES.info;

    const toast = document.createElement("div");
    toast.className = "notification";
    toast.style.setProperty("--notification-color", type.color);

    toast.innerHTML = `
        <span class="notification__icon"><img src=${type.icon}></img></span>
        <span class="notification__message">${message}</span>
        <div class="notification__progress"></div>
    `;

    notificationContainer.appendChild(toast);
    requestAnimationFrame(() => {
        requestAnimationFrame(() => {
            toast.classList.add("notification--visible");
        });
    });

    const progress = toast.querySelector(".notification__progress");
    progress.style.transition = `width ${duration}ms linear`;
    requestAnimationFrame(() => {
        requestAnimationFrame(() => {
            progress.style.width = "0%";
        });
    });

    setTimeout(() => {
        toast.classList.remove("notification--visible");
        toast.addEventListener("transitionend", () => toast.remove(), { once: true });
    }, duration);
}

notify(`Loaded ${workspace_path}`, "success", 1000);

// --- helper functions

async function loadNote(path) {
    if (!await exists(path)) { 
        console.log(`path incorrect or corrupted: ${path}`); 
        notify(`Failed to load file`, "error", 3000);
    } else {
        try {
            is_loading_entry = true;   
            const note_content = await readTextFile(path);
            current_editor_file = path;
            editor.value(note_content);
        }
        catch {
            console.log(`path failed to read ${path}, most likely a folder (ignore this in that case)`);
            notify(`Just tried to load a folder as a file...`, "warning", 1000);
        }
        finally {
            is_loading_entry = false;
        }
    }
}

async function saveNote(path) {
    if (!path) return;
    if (!await exists(path)) {
        console.log(`path ${path} isn't valid - incorrect or corrupted.`);
        notify(`Saving error: invalid path`, "error", 3000);
        return;
    }
    try {
        await writeTextFile(path, editor.value());
    } catch {
        console.log(`path failed to write ${path}`);
        notify(`Saving error: failed to write`, "error", 3000);
    }
}

async function renameEntry(newtitle) {
    if (last_selected_path) {
        const parts = last_selected_path.split(/[/\\]/);
        parts.pop();
        parts.push(newtitle);
        const newPath = parts.join("/");
        await rename(last_selected_path, newPath);
    }
}

async function deleteEntryFile() {
    if (last_selected_type !== "file") {
        console.log(`${last_selected_path} isn't a file.`);
        return;
    }
    await remove(last_selected_path);
    last_selected_path = null;
    last_selected_entry = null;
    last_selected_type = null;
}

async function deleteEntryFolder() {
    if (last_selected_type !== "directory") {
        console.log(`${last_selected_path} isn't a folder.`);
        return;
    }
    await remove(last_selected_path);
    last_selected_path = null;
    last_selected_entry = null;
    last_selected_type = null;
}

async function createEntryFile() {
    const name = prompt("Name the new file ...");
    let fullPath = await join(workspace_path, name);

    if (last_selected_path) {
        const parts = last_selected_path.split(/[/\\]/);

        if (last_selected_type == "file") {
            parts.pop();
        }

        parts.push(name);

        fullPath = parts.join("/");
    }

    const file = await create(fullPath);
    await file.close();
}

async function createEntryFolder() {
    const name = prompt("Name the new folder ...");
    let fullPath = await join(workspace_path, name);

    if (last_selected_path) {
        const parts = last_selected_path.split(/[/\\]/);

        parts.push(name);

        fullPath = parts.join("/");
    }

    await mkdir(fullPath);
}

async function renderWorkspaceTree(node) {
    const entry = document.createElement("div");
    const label = document.createElement("p");
    const label_icon = document.createElement("img");

    label.textContent = node.name;
    label.appendChild(label_icon);
    entry.className = "explorer-list__entry";
    entry.appendChild(label);

    label_icon.src = 'assets/svg/file-text.svg';

    label.addEventListener("click", async (event) => {
        event.preventDefault();
        if(last_selected_entry){
            last_selected_entry.id = "deselected-element";
        }
        last_selected_entry = label;
        last_selected_path = node.path;
        last_selected_type = node.is_dir ? "directory" : "file";
        label.id = "selected-element";
    });

    if (node.is_dir && node.children) {
        entry.className = "explorer-list__folder-container"
        label_icon.src = 'assets/svg/chevron-down.svg';

        const childrenContainer = document.createElement("div");
        childrenContainer.className = "explorer-list__folder-contents";

        label.addEventListener("click", async (event) => {
            event.preventDefault();
            if (childrenContainer.id=="collapsed-directory"){
                childrenContainer.id = "expanded-directory";
                label_icon.src = 'assets/svg/chevron-down.svg';
            } else {
                childrenContainer.id = "collapsed-directory";
                label_icon.src = 'assets/svg/chevron-right.svg';
            }
        });

        for (const child of node.children) {
            const childElement = await renderWorkspaceTree(child);
            childrenContainer.appendChild(childElement);
        }

        entry.appendChild(childrenContainer);
    } else {
        label.addEventListener("click", async (event) => {
            event.preventDefault();
            await loadNote(node.path);
        });
    }

    return entry;
}

function snapshotTree(node) {
    return JSON.stringify(node, (key, value) => {
        if (key === 'children') return value;
        if (key === 'name' || key === 'path' || key === 'is_dir') return value;
        return undefined;
    });
}

async function buildTree(nav_element) {
    const built_tree = await invoke("get_workspace_tree", { path: workspace_path });
    const snapshot = snapshotTree(built_tree);

    if (snapshot === last_tree_snapshot) return;

    last_tree_snapshot = snapshot;
    nav_element.innerHTML = "";
    nav_element.appendChild(await renderWorkspaceTree(built_tree));
}

// --- main functionality

const left_explorer_toolbar = document.getElementById("workspace-panel__left-explorer__toolbar");
const toolbar_add_note = document.getElementById("toolbar_add_note");
const toolbar_del_note = document.getElementById("toolbar_del_note");
const toolbar_add_dir = document.getElementById("toolbar_add_dir");
const toolbar_del_dir = document.getElementById("toolbar_del_dir");
const toolbar_rename = document.getElementById("toolbar_rename");

const left_panel_toolbar = document.getElementById("workspace-panel__left-toolbar");
const left_toolbar_home = document.getElementById("left-toolbar__home");
left_toolbar_home.addEventListener("click", async () => {
    window.location.href = `/index.html`;
});

const explorer_list = document.getElementById("workspace-panel__left-explorer__list");
await buildTree(explorer_list);
await watch(workspace_path, async () => {
    await buildTree(explorer_list);
});

toolbar_rename.addEventListener("click", async () => {
    const rename_text = prompt("Rename to ...");
    if(!rename_text){return};
    await renameEntry(rename_text);
    await buildTree(explorer_list);
});

toolbar_del_note.addEventListener("click", async () => {
    await deleteEntryFile();
    await buildTree(explorer_list);
});

toolbar_del_dir.addEventListener("click", async () => {
    await deleteEntryFolder();
    await buildTree(explorer_list);
});

toolbar_add_note.addEventListener("click", async () => {
    await createEntryFile();
    await buildTree(explorer_list);
});

toolbar_add_dir.addEventListener("click", async () => {
    await createEntryFolder();
    await buildTree(explorer_list);
});

let timeoutId;
let pendingSavePath = null;

function handleInput() {
    clearTimeout(timeoutId);
    pendingSavePath = current_editor_file;

    timeoutId = setTimeout(async () => {
        if (pendingSavePath) {
            await saveNote(pendingSavePath);
        }
    }, 250);
}

editor.codemirror.on("change", () => {
  handleInput();
});
