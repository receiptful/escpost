export const DAEMON_HOST = "127.0.0.1";

export const DAEMON_PORTS = [
  9000, 9001, 9002, 9003, 9004, 9005, 9006, 9007, 9008, 9009,
] as const;

export const DAEMON_BASE = `http://${DAEMON_HOST}:${DAEMON_PORTS[0]}`;
