const { invoke } = window.__TAURI__.core;
const { exists, BaseDirectory, mkdir, writeTextFile, readTextFile, watch } = window.__TAURI__.fs;
const { join } = window.__TAURI__.path;

const editor = new EasyMDE({element: document.getElementById('right-editor__area')});
let current_editor_file = "";

const searchParams = new URLSearchParams(window.location.search);
const workspace_path = searchParams.get("path");
let last_selected_entry;

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

async function buildWorkspaceTree() {
    // ...
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
        label.id = "selected-element";
        
        await loadNote(node.path);
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

const left_panel_toolbar = document.getElementById("workspace-panel__left-toolbar");
const left_toolbar_home = document.getElementById("left-toolbar__home");
left_panel_toolbar.addEventListener("click", async () => {
    window.location.href = `/index.html`;
})

const explorer_list = document.getElementById("workspace-panel__left-explorer__list");
await watchTree(explorer_list);
await watch(workspace_path, async () => {
    await watchTree(explorer_list);
})

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