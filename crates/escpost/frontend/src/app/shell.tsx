import type { ComponentChildren } from "preact";
import { Navigation } from "./navigation";
import { ServerStatus } from "./server-status";

type AppShellProps = {
  children: ComponentChildren;
};

export function AppShell({ children }: AppShellProps) {
  return (
    // The page holds the height of the window and gives what is left to the
    // main area, thus a page decides for itself what scrolls inside it.
    <div class="flex h-screen flex-col overflow-hidden bg-base-200 text-base-content">
      <aside class="fixed inset-y-0 left-0 hidden w-72 border-r border-base-300 bg-base-100 p-5 lg:flex lg:flex-col">
        <div>
          <p class="text-sm font-semibold tracking-wide text-primary">ESCPost</p>
          <h1 class="mt-1 text-xl font-bold">Workbench</h1>
        </div>
        <div class="mt-8">
          <Navigation />
        </div>
        <ServerStatus />
      </aside>
      <header class="border-b border-base-300 bg-base-100 px-4 py-2 lg:hidden">
        <ServerStatus compact />
      </header>
      <main class="flex min-h-0 flex-1 overflow-y-auto p-4 pb-24 sm:p-8 sm:pb-24 lg:ml-72 lg:p-10">
        <div class="flex min-h-0 w-full flex-1 flex-col">{children}</div>
      </main>
      <div class="fixed inset-x-0 bottom-0 z-10 border-t border-base-300 bg-base-100 lg:hidden">
        <Navigation mobile />
      </div>
    </div>
  );
}
