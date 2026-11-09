export interface CharterTenet {
  id: string; // e.g. "tenet-1"
  number: number;
  title: string;
  shortName: string;
  statement: string;
  enforcedBy: string[];
}

export const CHARTER_TENETS: CharterTenet[] = [
  {
    id: "tenet-1",
    number: 1,
    title: "Tenet 1 — Your files stay home (privacy)",
    shortName: "Your files stay home",
    statement:
      "Raw file contents and directory listings are never sent to any cloud service. Cloud models plan and verify from summaries and condensed catalogs only; heavy reading happens on local models.",
    enforcedBy: [
      "The privacy boundary in the pipeline",
      "Memory cloud-share flags",
      "Zero-raw-file-dump audit rule",
    ],
  },
  {
    id: "tenet-2",
    number: 2,
    title: "Tenet 2 — Nothing happens without you (human authority)",
    shortName: "Nothing happens without you",
    statement:
      "Writes, approvals, permission grants and degraded-mode choices belong to the human. No persistent permission creep: capability grants die with the task.",
    enforcedBy: [
      "Approval queue",
      "Grant matrix",
      "Task-scoped grants",
      "No-auto-run on defer resume",
    ],
  },
  {
    id: "tenet-3",
    number: 3,
    title: "Tenet 3 — Some things are always no (hard limits)",
    shortName: "Some things are always no",
    statement:
      "A small deny list cannot be talked into, requested via REQUEST steps, or granted under any posture.",
    enforcedBy: [
      "Hardcoded deny list checked before any grant or approval",
    ],
  },
  {
    id: "tenet-4",
    number: 4,
    title: "Tenet 4 — Honest limits (epistemics)",
    shortName: "Honest limits",
    statement:
      "Kodiak must know what it knows, show receipts (memory delivery chips), refuse to draft from assumed facts (REQUEST mandate), refuse destructive or irrelevant changes, and classify errors honestly instead of retrying hopelessly.",
    enforcedBy: [
      "Delivered-chars chips",
      "Rule 9",
      "Destructive-diff guard",
      "Quota-aware degradation",
    ],
  },
  {
    id: "tenet-5",
    number: 5,
    title: "Tenet 5 — Local first (design bias)",
    shortName: "Local first",
    statement:
      "Everything sensitive runs on your machine; the cloud is consulted only where its strengths matter (planning, verification) and only for minimal context.",
    enforcedBy: [
      "3-tier pipeline design",
      "Local fallback path",
      "Journal that never leaves the disk",
    ],
  },
  {
    id: "tenet-6",
    number: 6,
    title: "Tenet 6 — A paper trail for everything (auditability)",
    shortName: "A paper trail for everything",
    statement:
      "Every decision, grant, migration, cleaning, probe and provider call lands in the local journal, readable by you, sendable by no one.",
    enforcedBy: [
      "Append-only journal.jsonl",
      "Per-run diagnostic logs",
    ],
  },
];

export function getTenet(numberOrId: number | string): CharterTenet | undefined {
  if (typeof numberOrId === "number") {
    return CHARTER_TENETS.find((t) => t.number === numberOrId);
  }
  const clean = numberOrId.toLowerCase().replace(/[^a-z0-9]/g, "");
  return CHARTER_TENETS.find(
    (t) =>
      t.id.toLowerCase().replace(/[^a-z0-9]/g, "") === clean ||
      t.shortName.toLowerCase().replace(/[^a-z0-9]/g, "").includes(clean)
  );
}

/**
 * Generates an interactive pill/chip linking to a Charter tenet.
 * Clicking switches to the About tab and smoothly scrolls to the tenet.
 */
export function renderCharterChip(
  tenetNumber: number,
  labelPrefix?: string,
  extraClass = ""
): string {
  const tenet = getTenet(tenetNumber);
  if (!tenet) return "";
  const prefix = labelPrefix ? `${labelPrefix} — ` : "";
  return `<button type="button" class="charter-link-chip ${extraClass}" data-tenet-target="${tenet.id}" title="${tenet.statement} (Click to view Charter in About)">
    ${prefix}<strong>${tenet.shortName}</strong> <span class="charter-chip-icon">📖</span>
  </button>`;
}
