import { useLocation } from "preact-iso";

const destinations = [
  { href: "/", label: "Overview" },
  { href: "/jobs", label: "Print jobs" },
  { href: "/printers", label: "Printers" },
  { href: "/profiles", label: "Profiles" },
  { href: "/calibration", label: "Calibration" },
] as const;

type NavigationProps = {
  mobile?: boolean;
};

function isCurrentPath(path: string, href: string) {
  return path === href;
}

export function Navigation({ mobile = false }: NavigationProps) {
  const { path } = useLocation();
  const label = mobile ? "Mobile workbench navigation" : "Workbench navigation";

  return (
    <nav aria-label={label}>
      <ul class={mobile ? "grid grid-cols-5" : "menu gap-1 p-0"}>
        {destinations.map(({ href, label: destinationLabel }) => {
          const current = isCurrentPath(path, href);
          const className = mobile
            ? `flex min-h-16 flex-col items-center justify-center px-1 text-center text-xs ${current ? "bg-primary text-primary-content font-semibold" : ""}`
            : current ? "menu-active font-semibold" : "";
          return (
            <li key={href}>
              <a aria-current={current ? "page" : undefined} class={className} href={href}>
                {destinationLabel}
              </a>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
