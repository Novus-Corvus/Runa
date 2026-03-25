const { invoke } = window.__TAURI__.core;
const { exists, BaseDirectory, mkdir, writeTextFile, readTextFile, watch } = window.__TAURI__.fs;
const { join } = window.__TAURI__.path;

const searchParams = new URLSearchParams(window.location.search);
const workspace_path = searchParams.get("path");

// --- helper functions

async function buildWorkspaceTree() {
    // ...
}

async function renderWorkspaceTree(node) {
    const entry = document.createElement("div");

    // label (name only)
    const label = document.createElement("div");
    label.textContent = node.name;
    label.className = "explorer-list__entry";

    entry.appendChild(label);

    if (node.is_dir && node.children) {
        const childrenContainer = document.createElement("div");
        childrenContainer.className = "explorer-list__folder";

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