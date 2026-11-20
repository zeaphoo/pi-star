import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { PwaRegistration } from "@/components/PwaRegistration";
import { installFetchPolicy } from "@/lib/api-fetch";
import Home from "@/app/page";
import LoginPage from "@/app/login/page";
import "katex/dist/katex.min.css";
import "@/app/globals.css";
import "@/app/settings.css";
import "@/app/sidebar.css";
import "@/app/sidebar-menu.css";
import "@/app/trace.css";

// Every ordinary request gets a timeout, even one whose call site forgot:
// a full connection pool used to leave the UI spinning forever.
installFetchPolicy();

const root = document.getElementById("root");
if (!root) throw new Error("missing #root");

const page = window.location.pathname === "/login" ? <LoginPage /> : <Home />;

createRoot(root).render(
  <StrictMode>
    {page}
    <PwaRegistration />
  </StrictMode>,
);
