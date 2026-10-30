import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { PwaRegistration } from "@/components/PwaRegistration";
import Home from "@/app/page";
import LoginPage from "@/app/login/page";
import "katex/dist/katex.min.css";
import "@/app/globals.css";
import "@/app/settings.css";
import "@/app/sidebar.css";
import "@/app/sidebar-menu.css";

const root = document.getElementById("root");
if (!root) throw new Error("missing #root");

const page = window.location.pathname === "/login" ? <LoginPage /> : <Home />;

createRoot(root).render(
  <StrictMode>
    {page}
    <PwaRegistration />
  </StrictMode>,
);
