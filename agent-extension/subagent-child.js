/**
 * Runs inside a subagent's own `pi --mode rpc` process.
 *
 * Its only job is to make the child session self-describing: the metadata that
 * links it to its parent, and a status every transition updates. The parent
 * extension writes the final status/result once the process exits, so this file
 * never has to guess whether a run completed or failed.
 */
export default function piStarSubagentChild(pi) {
    let meta = null;
    try {
        meta = process.env.PI_STAR_SUBAGENT_META ? JSON.parse(process.env.PI_STAR_SUBAGENT_META) : null;
    }
    catch {
        meta = null;
    }
    if (!meta)
        return;
    // appendEntry persists the file even before the first message, which is what
    // makes a queued or failed run visible in the sidebar at all.
    const startQueued = process.env.PI_STAR_SUBAGENT_START_QUEUED === "1";
    pi.appendEntry("pi-star:subagent", meta);
    pi.appendEntry("pi-star:subagent-status", { version: 1, status: startQueued ? "queued" : "running" });
    // A queued child was spawned without its prompt; the parent sends it when a
    // slot frees, and this is the first moment the file can say "running".
    let announced = !startQueued;
    pi.on("agent_start", () => {
        if (announced)
            return;
        announced = true;
        pi.appendEntry("pi-star:subagent-status", { version: 1, status: "running" });
    });
}
