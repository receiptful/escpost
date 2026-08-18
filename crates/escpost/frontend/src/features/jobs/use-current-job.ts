import { useEffect, useState } from "preact/hooks";
import { getCurrentJob } from "../../api/client";
import type { CurrentJobResponse } from "../../api/types";

export type CurrentJobResource = {
  data: CurrentJobResponse | null;
  error: Error | null;
  loading: boolean;
};

export function useCurrentJob(pollInterval = 750): CurrentJobResource {
  const [resource, setResource] = useState<CurrentJobResource>({
    data: null,
    error: null,
    loading: true,
  });

  useEffect(() => {
    let active = true;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    let controller: AbortController | null = null;

    const poll = () => {
      controller = new AbortController();
      void getCurrentJob(controller.signal)
        .then((next) => {
          if (!active) return;
          setResource((current) => {
            const sameJob = current.data?.job?.id && current.data.job.id === next.job?.id;
            return {
              data: sameJob ? { ...next, job: current.data?.job ?? next.job } : next,
              error: null,
              loading: false,
            };
          });
        })
        .catch((error: unknown) => {
          if (!active || controller?.signal.aborted) return;
          setResource((current) => ({
            data: current.data,
            error: error instanceof Error ? error : new Error("Job data is unavailable."),
            loading: false,
          }));
        })
        .finally(() => {
          if (active) timeout = setTimeout(poll, pollInterval);
        });
    };

    poll();
    return () => {
      active = false;
      if (timeout !== undefined) clearTimeout(timeout);
      controller?.abort();
    };
  }, [pollInterval]);

  return resource;
}
