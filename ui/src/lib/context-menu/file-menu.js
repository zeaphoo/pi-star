import { copyText } from "@/lib/clipboard";

/**
 * The two copy actions shared by Git change and commit file rows.
 *
 * @param {object} args
 * @param {string} args.absolute
 * @param {string} args.relative
 * @param {(key: string) => string} args.t
 * @returns {import("./types").ContextMenuEntry[]}
 */
export function filePathMenuItems({ absolute, relative, t }) {
  return [
    {
      id: "copy-relative",
      label: t("menu.copyRelativePath"),
      onSelect: () => {
        void copyText(relative);
      },
    },
    {
      id: "copy-absolute",
      label: t("menu.copyAbsolutePath"),
      onSelect: () => {
        void copyText(absolute);
      },
    },
  ];
}
