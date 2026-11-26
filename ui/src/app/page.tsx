import { Suspense } from "react";
import { AppShell } from "@/components/AppShell";
import { I18nProvider } from "@/hooks/useI18n";
import { ContextMenuProvider } from "@/hooks/ContextMenuProvider.jsx";

export default function Home() {
  return (
    <Suspense>
      <I18nProvider>
        <ContextMenuProvider>
          <AppShell />
        </ContextMenuProvider>
      </I18nProvider>
    </Suspense>
  );
}
