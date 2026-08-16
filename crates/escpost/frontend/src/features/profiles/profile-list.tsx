import { Fragment } from "preact";
import type { Profile } from "../../api/types";
import { useAppData } from "../../app/data";

const columns = ["PROFILE", "VENDOR", "MODEL", "CAL", "PAPER", "PRINT", "DOTS", "DPI", "CUT", "BC", "QR"] as const;

function checkMarker(supported: boolean) {
  return supported ? "✓" : "–";
}

function sourceMarker(profile: Profile) {
  switch (profile.source) {
    case "calibrated": return "✓";
    case "synthesized": return "~";
    case "virtual": return "○";
  }
}

function barcodeMarker(profile: Profile) {
  if (profile.barcode_function_a && profile.barcode_function_b) return "A·B";
  if (profile.barcode_function_a) return "A";
  if (profile.barcode_function_b) return "B";
  return "–";
}

function fields(profile: Profile) {
  return [
    profile.id,
    profile.vendor,
    profile.model,
    sourceMarker(profile),
    profile.paper_width_mm.toFixed(1),
    profile.printable_width_mm.toFixed(1),
    profile.printable_width_dots.toString(),
    profile.dpi_x.toString(),
    checkMarker(profile.full_cut || profile.partial_cut),
    barcodeMarker(profile),
    checkMarker(profile.qr_code),
  ];
}

export function ProfileList() {
  const { profiles, refreshProfiles } = useAppData();
  const profileData = profiles.data?.profiles;

  if (!profileData) {
    if (profiles.phase === "error") {
      return (
        <section class="rounded-box bg-base-100 p-5 shadow-sm" aria-live="polite">
          <p>{profiles.error?.message ?? "Unable to load profile catalog."}</p>
          <button class="btn btn-primary mt-4" type="button" onClick={() => void refreshProfiles()}>Retry</button>
        </section>
      );
    }
    return <p aria-live="polite" class="text-base-content/70">Loading profiles…</p>;
  }

  return (
    <div class="space-y-4">
      {profiles.phase === "refreshing" && <p aria-live="polite" class="text-sm text-base-content/70">Refreshing profiles…</p>}
      {profiles.error && <p role="alert" class="alert alert-warning">Showing cached profile data. {profiles.error.message}</p>}
      {profileData.length === 0 ? (
        <section class="rounded-box bg-base-100 p-5 shadow-sm"><p>No profiles available.</p></section>
      ) : <>
        <div class="hidden overflow-x-auto rounded-box bg-base-100 shadow-sm lg:block">
          <table class="table">
            <thead><tr>{columns.map((column) => <th key={column}>{column}</th>)}</tr></thead>
            <tbody>{profileData.map((profile) => <tr key={profile.id}>{fields(profile).map((value, index) => <td key={columns[index]}>{value}</td>)}</tr>)}</tbody>
          </table>
        </div>
        <div class="space-y-3 lg:hidden">
          {profileData.map((profile) => <article key={profile.id} class="rounded-box bg-base-100 p-5 shadow-sm">
            <dl class="grid grid-cols-2 gap-3 text-sm">
              {fields(profile).map((value, index) => <Fragment key={columns[index]}><dt class="font-medium text-base-content/70">{columns[index]}</dt><dd>{value}</dd></Fragment>)}
            </dl>
          </article>)}
        </div>
        <p class="text-sm text-base-content/70">CAL: ✓ calibrated · ~ synthesized · ○ virtual   PAPER/PRINT mm, DOTS printable</p>
      </>}
    </div>
  );
}
