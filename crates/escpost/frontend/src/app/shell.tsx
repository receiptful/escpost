import type { ComponentChildren } from "preact";
import { ConnectionStatus } from "./connection-status";
import { Navigation } from "./navigation";

type AppShellProps = {
  children: ComponentChildren;
};

export function AppShell({ children }: AppShellProps) {
  return (
    <div class="min-h-screen bg-base-200 text-base-content">
      <aside class="fixed inset-y-0 left-0 hidden w-72 border-r border-base-300 bg-base-100 p-5 lg:flex lg:flex-col">
        <div>
          <p class="text-sm font-semibold tracking-wide text-primary">ESCPost</p>
          <h1 class="mt-1 text-xl font-bold">Workbench</h1>
        </div>
        <div class="mt-8">
          <Navigation />
        </div>
        <ConnectionStatus />
      </aside>
      <header class="border-b border-base-300 bg-base-100 px-4 py-2 lg:hidden">
        <ConnectionStatus compact />
      </header>
      <main class="flex min-h-screen p-4 pb-24 sm:p-8 sm:pb-24 lg:ml-72 lg:p-10">
        <div class="flex w-full flex-col">{children}</div>
      </main>
      <div class="fixed inset-x-0 bottom-0 z-10 border-t border-base-300 bg-base-100 lg:hidden">
        <Navigation mobile />
      </div>
    </div>
  );
}
