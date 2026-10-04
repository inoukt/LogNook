export function reconcileSxPorts(availablePorts: string[], selectedPort: string) {
  const ports = [...new Set(availablePorts)].sort();
  const nextSelectedPort = ports.includes(selectedPort) ? selectedPort : ports[0] ?? "";
  return {
    ports,
    selectedPort: nextSelectedPort,
    selectionChanged: nextSelectedPort !== selectedPort,
  };
}
