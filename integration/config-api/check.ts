import { listenPort } from "./main.ts"

if (import.meta.main) {
    try {
        await fetch(`http://127.0.0.1:${listenPort}`)
    } catch {
        Deno.exit(1)
    }
}
