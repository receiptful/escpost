import { useAppData } from "../../app/data";
import { useEffect } from "preact/hooks";
import { ProfileList } from "./profile-list";

export function ProfilesPage() {
  const { profiles, ensureProfiles, refreshProfiles } = useAppData();
  useEffect(() => {
    void ensureProfiles();
  }, [ensureProfiles]);
  return (
    <section aria-labelledby="profiles-heading" class="space-y-6">
      <div class="flex flex-wrap items-end justify-between gap-4">
        <div>
          <p class="text-sm font-semibold text-primary">Workbench</p>
          <h1 id="profiles-heading" class="mt-1 text-3xl font-bold">Profiles</h1>
        </div>
        <button class="btn btn-primary" type="button" onClick={() => void refreshProfiles()} disabled={profiles.phase === "loading" || profiles.phase === "refreshing"}>
          Refresh
        </button>
      </div>
      <ProfileList />
    </section>
  );
}
