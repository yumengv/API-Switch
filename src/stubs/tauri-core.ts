export async function invoke(_cmd: string, _args?: Record<string, unknown>): Promise<unknown> {
  throw new Error("Tauri invoke is not available in web mode");
}
