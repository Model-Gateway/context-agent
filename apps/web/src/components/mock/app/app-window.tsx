import { cn } from "@/lib/cn";
import { ChatPane } from "./chat-pane";
import { Inspector } from "./inspector";
import { Sidebar } from "./sidebar";
import { TitleBar } from "./title-bar";

/**
 * A faithful re-creation of the Context macOS app in the middle of an Ultra
 * run: two explore agents have reported on how edits are protected and the
 * answer is streaming in. It is an illustration, so it is exposed as a
 * single image with a description and its contents are hidden from
 * assistive technology.
 *
 * Give it a height through `className`; the panes clip their content, so a
 * parent can crop the window at any height. Below `lg` the inspector is
 * hidden, and below `md` the sidebar too.
 */
export function AppWindow({ className }: { className?: string }) {
  return (
    <div
      role="img"
      aria-label="The Context app for macOS during a run: a question about safe edits, two explore agents that investigated it in parallel and finished, the answer streaming in, and an inspector showing the running task and the assembled context."
      className={cn("overflow-hidden rounded-[10px] bg-surface font-sans ring-1 ring-black/[0.06]", className)}
    >
      <div aria-hidden className="flex h-full select-none flex-col">
        <TitleBar />
        <div className="flex min-h-0 flex-1">
          <Sidebar />
          <ChatPane />
          <Inspector />
        </div>
      </div>
    </div>
  );
}
