import type { ImgHTMLAttributes } from "react";

type NextImageProps = Omit<ImgHTMLAttributes<HTMLImageElement>, "src"> & {
  src: string;
  width?: number;
  height?: number;
  priority?: boolean;
  fill?: boolean;
  unoptimized?: boolean;
};

/** next/image is used only for two static logos; a plain img keeps the UI identical. */
export default function Image({ priority, fill, unoptimized, width, height, style, ...props }: NextImageProps) {
  void priority;
  void fill;
  void unoptimized;
  return <img {...props} width={width} height={height} style={style} />;
}
