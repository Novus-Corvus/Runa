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
let is_saving_entry = null;
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

function notify(message, context = ``, severity = "info", duration = 1000) {
    if (context) {
        console[severity === "error" ? "error" : severity === "warning" ? "warn" : "log"](message, context);
    }
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

notify(`Loaded ${workspace_path}`, `Successfully loaded ${workspace_path} as a workspace`, "success", 1000);

// --- helper functions

async function loadNote(path) { // loads selected note from filesystem to editor
    if (!await exists(path)) {
        // notify the user if there's a problem
        // and give extra context in-console
        notify(
            `Failure loading file`, 
            `File loading path isn't valid or corrupted: ${path}`, 
            "error", 
            3000
        );
    } else {
        try { // Attempt opening and reading the file
            is_loading_entry = true;
            const note_content = await readTextFile(path);
            // set current file to passed path argument
            // we already have set type variable from clicking the entry
            current_editor_file = path;
            editor.value(note_content);
        }
        catch { // Resolve any errors
            // notify the user if there's a problem
            // and give extra context in-console
            notify(
                `Failure reading file`, 
                `File couldn't be read: ${path}; Either corrupt file or mistakenly passed directory path.`, 
                "error", 
                3000
            );
        }
        finally { // Cleanup
            is_loading_entry = false;
        }
    }
}

async function saveNote(path) { // saves current editor note to filesystem
    // first of all, check if path is even real
    if (!path) return;
    // validate path's existance
    if (!await exists(path)) {
        // notify the user if there's a problem
        // and give extra context in-console
        notify(
            `Failure saving file`,
            `File couldn't be saved: ${path}; Invalid filesystem path or corrupt file.`,
            "error",
            3000
        );
        return;
    } else {
        try { // Attempt saving to file
            is_saving_entry = true;
            await writeTextFile(path, editor.value());
        }
        catch { // Resolve any errors
            // notify the user if there's a problem
            // and give extra context in-console
            notify(
                `Failure saving file`,
                `File couldn't be written to: ${path}; Invalid filesystem path or corrupt file.`,
                "error",
                3000
            );
        }
        finally { // Cleanup
            is_saving_entry = false;
        }
    }
}

async function renameEntry(title) { // Renames file/folder
    // check if path exists
    if (!last_selected_path) return;
    // validate path's existance
    if (!await exists(last_selected_path)) {
        // notify the user if there's a problem
        // and give extra context in-console
        notify(
            `Invalid file/folder`,
            `Entry couldn't be renamed: ${path}; Invalid filesystem path or corrupt file/folder.`,
            "error",
            3000
        );
        return;
    } else {
        try { // Attempt renaming
            // split the path into sections
            const parts = last_selected_path.split(/[/\\]/);
            // remove the name of the file/folder
            parts.pop();
            // put in a new name on the end
            parts.push(title);
            // join all sections back into a path
            const new_path = parts.join("/");
            // call rename with new path
            await rename(last_selected_path, new_path);
        }
        catch { // Resolve any errors
            // notify the user if there's a problem
            // and give extra context in-console
            notify(
                `Invalid file/folder`,
                `Entry couldn't be renamed: ${path}; Invalid filesystem path or corrupt file/folder.`,
                "error",
                3000
            );
        }
        finally {
            notify(
                `Renamed ${last_selected_path}`,
                ``,
                "success",
                1000
            );
        }
    }
}

async function deleteEntryFile() {
    // check if path exists
    if (!last_selected_path) return;
    if (last_selected_type!=="file") return;
    // validate path's existance
    if (!await exists(last_selected_path)) {
        // notify the user if there's a problem
        // and give extra context in-console
        notify(
            `Invalid file/folder`,
            `Entry couldn't be renamed: ${path}; Invalid filesystem path or corrupt file/folder.`,
            "error",
            3000
        );
        return;
    } else {
        try { // Attempt removal
            await remove(last_selected_path);
        }
        catch { // Resolve any errors
            // notify the user if there's a problem
            // and give extra context in-console
            notify(
                `Invalid file/folder`,
                `Path couldn't be deleted: ${last_selected_path}; Invalid filesystem path or corrupt file/folder.`,
                "error",
                3000
            );
        }
        finally {
            last_selected_path = null;
            last_selected_entry = null;
            last_selected_type = null;
            notify(
                `Deleted ${last_selected_path}`,
                ``,
                "success",
                1000
            );
        }
    }
}

async function deleteEntryFolder() {
    // check if path exists
    if (!last_selected_path) return;
    if (last_selected_type!=="directory") return;
    // validate path's existance
    if (!await exists(last_selected_path)) {
        // notify the user if there's a problem
        // and give extra context in-console
        notify(
            `Invalid file/folder`,
            `Entry couldn't be renamed: ${path}; Invalid filesystem path or corrupt file/folder.`,
            "error",
            3000
        );
        return;
    } else {
        try { // Attempt removal
            await remove(last_selected_path);
        }
        catch { // Resolve any errors
            // notify the user if there's a problem
            // and give extra context in-console
            notify(
                `Invalid file/folder`,
                `Path couldn't be deleted: ${last_selected_path}; Invalid filesystem path or corrupt file/folder.`,
                "error",
                3000
            );
        }
        finally {
            last_selected_path = null;
            last_selected_entry = null;
            last_selected_type = null;
            notify(
                `Deleted ${last_selected_path}`,
                ``,
                "success",
                1000
            );
        }
    }
}

async function createEntryFile() { // Creates new file
    // temporary stand-in prompt for filename
    const name = prompt("Name the new file ...");
    // make the new path from the root workspace path
    let full_path = await join(workspace_path, name);
    // If current path exists, find directory and make the file there instead
    if (last_selected_path && last_selected_type) {
        if (await exists(last_selected_path)) {
            try {
                // split the path into sections
                const parts = last_selected_path.split(/[/\\]/);
                // remove the name of the file/folder
                if (last_selected_type == "file") {
                    parts.pop();
                }
                // put in a new name on the end
                parts.push(name);
                // join all sections back into a path
                full_path = parts.join("/");
            }
            catch {
                // notify the user if there's a problem
                // and give extra context in-console
                notify(
                    `Invalid path`,
                    `Path couldn't be constructed: ${last_selected_path}; Invalid filesystem path or corrupt file/folder.`,
                    "error",
                    3000
                );
            }
        }
    }
    
    // Continuing ...
    try { // Attempt creation
        const file = await create(full_path);
        await file.close();
    }
    catch { // Resolve any errors
        notify(
            `Failure creating file`,
            `File couldn't be created: ${full_path}; Invalid filesystem path or corrupt file/folder.`,
            "error",
            3000
        );
    }
}

async function createEntryFolder() { // Creates new folder
    // temporary stand-in prompt for folder name
    const name = prompt("Name the new folder ...");
    // make the new path from the root workspace path
    let full_path = await join(workspace_path, name);
    // If current path exists, find directory and nest the directory there instead
    if (last_selected_path && last_selected_type) {
        if (await exists(last_selected_path)) {
            try {
                // split the path into sections
                const parts = last_selected_path.split(/[/\\]/);
                // get rid of filename
                if (last_selected_type == "file") {
                    parts.pop();
                }
                // put in a new name on the end
                parts.push(name);
                // join all sections back into a path
                full_path = parts.join("/");
            }
            catch {
                // notify the user if there's a problem
                // and give extra context in-console
                notify(
                    `Invalid path`,
                    `Path couldn't be constructed: ${last_selected_path}; Invalid filesystem path or corrupt file/folder.`,
                    "error",
                    3000
                );
            }
        }
    }
    
    // Continuing ...
    try { // Attempt creation
        await mkdir(full_path);
    }
    catch { // Resolve any errors
        notify(
            `Failure creating folder`,
            `Folder couldn't be created: ${full_path}; Invalid filesystem path or corrupt file/folder.`,
            "error",
            3000
        );
    }
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
    return JSON.stringify(node);
}

async function buildTree(nav_element) {
    const built_tree = await invoke("get_workspace_tree", { path: workspace_path });
    const snapshot = snapshotTree(built_tree);

    if (snapshot === last_tree_snapshot) { 
        return;
    }

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
