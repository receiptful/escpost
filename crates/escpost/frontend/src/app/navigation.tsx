import { useLocation } from "preact-iso";

const destinations = [
  { href: "/app/", label: "Overview" },
  { href: "/app/jobs", label: "Print jobs" },
  { href: "/app/printers", label: "Printers" },
  { href: "/app/profiles", label: "Profiles" },
  { href: "/app/calibration", label: "Calibration" },
] as const;

type NavigationProps = {
  mobile?: boolean;
};

function isCurrentPath(path: string, href: string) {
  return path === href || (href === "/app/" && path === "/app");
}

export function Navigation({ mobile = false }: NavigationProps) {
  const { path } = useLocation();
  const label = mobile ? "Mobile workbench navigation" : "Workbench navigation";

  return (
    <nav aria-label={label}>
      <ul class={mobile ? "grid grid-cols-5" : "menu gap-1 p-0"}>
        {destinations.map(({ href, label: destinationLabel }) => (
          <li key={href}>
            <a
              aria-current={isCurrentPath(path, href) ? "page" : undefined}
              class={mobile ? "flex min-h-16 flex-col items-center justify-center px-1 text-center text-xs" : ""}
              href={href}
            >
              {destinationLabel}
            </a>
          </li>
        ))}
      </ul>
    </nav>
  );
}
