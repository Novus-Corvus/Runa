const { invoke } = window.__TAURI__.core;
const { open } = window.__TAURI__.dialog;

async function get_folder() {
  const file = await open({
    multiple: false,
    directory: true,
  });
  return file;
}

async function create_workspace(input_field) {
  const workspace_creation_directory = input_field.value;
  // ...
}

const workspaces_locate_btn = document.getElementById("local-workspaces__locate");
const workspaces_name_input = document.getElementById("local-workspaces__name");
const workspaces_done_btn   = document.getElementById("local-workspaces__done");
workspaces_locate_btn.addEventListener("click", async (event) => {
  event.preventDefault();
  const workspace_directory = await get_folder();
  workspaces_name_input.value = workspace_directory;
})

workspaces_done_btn.addEventListener("click", async (event) => {
  event.preventDefault();
  await create_workspace(workspaces_name_input);
})