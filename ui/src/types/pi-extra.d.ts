declare module "node-pty" {
  export interface IPty {
    write(data: string): void;
    resize(cols: number, rows: number): void;
    kill(signal?: string): void;
    onData(cb: (data: string) => void): void;
    onExit(cb: (event: { exitCode: number }) => void): void;
  }
  export function spawn(file: string, args: string[], options: Record<string, unknown>): IPty;
}
