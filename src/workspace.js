const { invoke } = window.__TAURI__.core;
const { exists, BaseDirectory, mkdir, writeTextFile, readTextFile, watch , rename, remove, stat, open, create} = window.__TAURI__.fs;
const { join } = window.__TAURI__.path;

const editor = new EasyMDE({element: document.getElementById('right-editor__area')});
let current_editor_file = "";

const searchParams = new URLSearchParams(window.location.search);
const workspace_path = searchParams.get("path");
let last_selected_entry;
let last_selected_path;

// --- helper functions

async function loadNote(path) {
    if (!await exists(path)) { console.log(`path incorrect or corrupted: ${path}`);
    } else {
        try {           
            const note_content = await readTextFile(path);
            current_editor_file = path;
            editor.value(note_content);
        }
        catch {
            console.log(`path failed to read ${path}, most likely a folder (ignore this in that case)`);
        }
    }
}

async function saveNote(path) {
    if (current_editor_file) {
        console.log(`saving ${current_editor_file}`);
        if (!await exists(current_editor_file)) { console.log(`path incorrect or corrupted: ${current_editor_file}`);
        } else {
            try {
                await writeTextFile(current_editor_file, editor.value());
            }
            catch {
                console.log(`path failed to write ${current_editor_file}`);
            }
        }
    }
}

async function checkEntry(path) {
    if (path) {
        let file;
        try {
            file = await open(path);
        } catch {
            return "dir";
        }
        const fileInfo = await file.stat();
        if (fileInfo.isFile == true) {
            await file.close();
            return "file";
        } else {
            await file.close();
            return "dir";
        }
    } else {
        return `path ${path} isn't valid.`;
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
    if (last_selected_path) {
        const type = await checkEntry(last_selected_path);
        if (type == "file") {
            console.log(`deleting ${last_selected_path}`);
            
            await remove(last_selected_path);
            last_selected_path = null;
            last_selected_entry = null;
        } else {
            console.log(`path ${last_selected_path} isn't a file`);
        }
    } else {
        console.log(`path ${last_selected_path} isn't valid.`);
    }
}

async function deleteEntryFolder() {
    if (last_selected_path) {
        const type = await checkEntry(last_selected_path);
        if (type == "dir") {
            console.log(`deleting ${last_selected_path}`);
            
            await remove(last_selected_path);
            last_selected_path = null;
            last_selected_entry = null;
        } else {
            console.log(`path ${last_selected_path} isn't a folder`);
        }
    } else {
        console.log(`path ${last_selected_path} isn't valid.`);
    }
}

async function createEntryFile() {
    const name = prompt("Name the new file ...");
    const fullPath = await join(workspace_path, name);
    const file = await create(fullPath);
    await file.close();
}

async function createEntryFolder() {
    const name = prompt("Name the new folder ...");
    const fullPath = await join(workspace_path, name);
    await mkdir(fullPath);
}

async function renderWorkspaceTree(node) {
    const entry = document.createElement("div");

    const label = document.createElement("p");
    label.textContent = node.name;
    entry.className = "explorer-list__entry";
    entry.appendChild(label);

    label.addEventListener("click", async (event) => {
        event.preventDefault();
        if(last_selected_entry){
            last_selected_entry.id = "deselected-element";
        }
        last_selected_entry = label;
        last_selected_path = node.path;
        label.id = "selected-element";
    });

    if (node.is_dir && node.children) {
        entry.className = "explorer-list__folder-container"

        const childrenContainer = document.createElement("div");
        childrenContainer.className = "explorer-list__folder-contents";

        label.addEventListener("click", async (event) => {
            event.preventDefault();
            if (childrenContainer.id=="collapsed-directory"){
                childrenContainer.id = "expanded-directory";
            } else {
                childrenContainer.id = "collapsed-directory";
            }
        });

        for (const child of node.children) {
            const childElement = await renderWorkspaceTree(child);
            childrenContainer.appendChild(childElement);
        }

        entry.appendChild(childrenContainer);
    } else {
        label.addEventListener("click", async (event) => {
            await loadNote(node.path);
        });
    }

    return entry;
}

async function watchTree(nav_element) {
    const built_tree = await invoke("get_workspace_tree", {
        path: workspace_path
    });
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
await watchTree(explorer_list);
await watch(workspace_path, async () => {
    await watchTree(explorer_list);
});

toolbar_rename.addEventListener("click", async () => {
    const rename_text = prompt("Rename to ...");
    if(!rename_text){return};
    await renameEntry(rename_text);
    await watchTree(explorer_list);
});

toolbar_del_note.addEventListener("click", async () => {
    await deleteEntryFile();
    await watchTree(explorer_list);
});

toolbar_del_dir.addEventListener("click", async () => {
    await deleteEntryFolder();
    await watchTree(explorer_list);
});

toolbar_add_note.addEventListener("click", async () => {
    await createEntryFile();
    await watchTree(explorer_list);
});

toolbar_add_dir.addEventListener("click", async () => {
    await createEntryFolder();
    await watchTree(explorer_list);
});

let timeoutId;

function handleInput() {
  clearTimeout(timeoutId);

  timeoutId = setTimeout(async () => {
    await saveNote();
  }, 250);
}

editor.codemirror.on("change", () => {
  handleInput();
});