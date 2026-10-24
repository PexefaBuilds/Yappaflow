// Which OS the UI runs on, for per-OS wording (⌘V vs Ctrl+V, Keychain vs Credential Manager).
export const isMac = /Mac/i.test(navigator.userAgent);
export const PASTE = isMac ? "⌘V" : "Ctrl+V";
