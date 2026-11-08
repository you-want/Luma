// Public IPC facade. Every Tauri command wrapper lives in `lib/ipc/<domain>`
// and is re-exported here, so components keep a single, stable import path
// (`lib/tauri`) while the implementations stay domain-scoped.
//
// When adding a command: put the wrapper in the matching `ipc/` module (or a
// new one) and re-export it here. Do not call `invoke` outside `lib/ipc/`.

export * from "./ipc/scan";
export * from "./ipc/fileManager";
export * from "./ipc/organize";
export * from "./ipc/cleanup";
export * from "./ipc/cleaner";
export * from "./ipc/system";
export * from "./ipc/storage";
export * from "./ipc/sessions";
