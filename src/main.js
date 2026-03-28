const { invoke } = window.__TAURI__.core;
const { open } = window.__TAURI__.dialog;
const { exists, BaseDirectory, mkdir, writeTextFile, readTextFile, create } = window.__TAURI__.fs;
const { join } = window.__TAURI__.path;

// --- manifest templates

const manifest_template_global = JSON.stringify(
  {
    "manifest_version": "0.1.0",
    "workspaces": []
  }
);

const manifest_template_local = JSON.stringify(
  {
    "uuid": "",
    "manifest_version": "0.1.0",
    "workspace_name": "Template",
    "preferences": {}
  }
);

// --- helper functions

async function getFolderDialog() {
  const file = await open({
    multiple: false,
    directory: true,
  });
  return file;
}

async function getWorkspacesManifest() {
  // Ensure Appdata entry exists
  await mkdir('', { 
    baseDir: BaseDirectory.AppConfig, 
    recursive: true 
  });
  // Check for manifest
  if (!await exists('manifest.json', {
    baseDir: BaseDirectory.AppConfig,
  })) {
    // No manifest, create new one from template
    await writeTextFile('manifest.json', manifest_template_global, {
      baseDir: BaseDirectory.AppConfig
    });
    return JSON.parse(manifest_template_global);
  }
  // Return manifest contents
  return JSON.parse(await readTextFile('manifest.json', {
    baseDir: BaseDirectory.AppConfig,
  }));
}

async function addManifestPath(filepath) {
  const manifest_file = 'manifest.json';
  let manifest;

  // Check if manifest exists
  if (!await exists(manifest_file, { baseDir: BaseDirectory.AppConfig })) {
    // Create manifest from template if it doesn't exist
    await writeTextFile(manifest_file, manifest_template_global, { baseDir: BaseDirectory.AppConfig });
    manifest = JSON.parse(manifest_template_global);
  } else {
    // Read existing manifest
    manifest = JSON.parse(await readTextFile(manifest_file, { baseDir: BaseDirectory.AppConfig }));
  }

  // Ensure the path is only added once
  if (!manifest.workspaces.includes(filepath)) {
    manifest.workspaces.push(filepath);
    // Save updated manifest
    await writeTextFile(manifest_file, JSON.stringify(manifest, null, 2), { baseDir: BaseDirectory.AppConfig });
  }

  return manifest;
}

async function getWorkspaceManifest(filepath) {
  const manifest_filename = "manifest.runa";
  const manifest_filepath = await join(filepath, manifest_filename);
  
  // Find workspace path and manifest
  if (!await exists(manifest_filepath)) { return null;
  } else {
    const manifest_content = await readTextFile(manifest_filepath);
    return JSON.parse(manifest_content);
  }
}

async function enterWorkspace(workspace_path) {
  console.log(workspace_path);
  window.location.href = `/workspace.html?path=${workspace_path}`;
}

async function buildWorkspaceEntry(manifest_content, workspace_path) {
  // construct entry element
  const div = document.createElement("div");
  div.id = "workspace-entry";

  const title = document.createElement("p");
  title.id = "workspace-entry__title";
  title.textContent = manifest_content["workspace_name"];

  const img = document.createElement("img");
  img.id = "workspace-entry__settings"
  img.src = "assets/svg/settings.svg";
  img.alt = "workspace settings";

  div.appendChild(title);
  div.appendChild(img);
  div.addEventListener("click", async (event) => {
    event.preventDefault();
    await enterWorkspace(workspace_path);
  });
  return div;
}

async function buildWorkspacesList() {
  // get all workspaces from manifest
  const manifest_json = await getWorkspacesManifest();
  const workspaces_list = manifest_json["workspaces"];
  const parent_element = document.getElementById("local-workspaces__nav");
  parent_element.innerHTML = "";

  for (let i = 0; i < workspaces_list.length; i++) {
    // get each workspace
    const manifest_content = await getWorkspaceManifest(workspaces_list[i]);
    if (manifest_content === null) {console.log("Invalid workspace path."); continue};
    console.log(`Got ${workspaces_list[i]} workspace titled '${manifest_content["workspace_name"]}'`);

    // create workspace elements ...
    const workspace_entry = await buildWorkspaceEntry(manifest_content, workspaces_list[i]);
    parent_element.appendChild(workspace_entry);
  }
}

async function createWorkspace(input_field) {
  // Creates a new workspace in a directory
  if (input_field.value === ""){return};
  const workspace_creation_directory = input_field.value;
  if (await exists(workspace_creation_directory)) {
    try {
      const workspace_creation_path = await join(workspace_creation_directory, "manifest.runa");
      const file = await create(workspace_creation_path);
      await file.close();
      await writeTextFile(workspace_creation_path, manifest_template_local);
    }
    catch {
      console.log(`failed writing new manifest to ${workspace_creation_directory}`);
    }
    finally {
      await addManifestPath(workspace_creation_directory);
      await buildWorkspacesList();
    }
  } else {
    console.log(`failed finding the selected directory ${workspace_creation_directory}`);
  }
}

// --- main functionality

await buildWorkspacesList();

const workspaces_locate_btn = document.getElementById("local-workspaces__locate");
const workspaces_name_input = document.getElementById("local-workspaces__name");
const workspaces_done_btn   = document.getElementById("local-workspaces__done");

workspaces_locate_btn.addEventListener("click", async (event) => {
  event.preventDefault();
  const workspace_directory = await getFolderDialog();
  workspaces_name_input.value = workspace_directory;
})

workspaces_done_btn.addEventListener("click", async (event) => {
  event.preventDefault();
  await createWorkspace(workspaces_name_input);
})