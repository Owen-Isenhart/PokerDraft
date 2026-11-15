export async function api<T>(path: string): Promise<T> {
  const response = await fetch(path);
  const data = await response.json();
  if (!data.ok) throw new Error(data.error ?? "request_failed");
  return data.result as T;
}
export async function command<T = Record<string, unknown>>(
  op: string,
  fields: Record<string, unknown> = {},
  token = "",
): Promise<T> {
  const response = await fetch("/api/command", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: JSON.stringify({ op, id: crypto.randomUUID(), ...fields }),
  });
  const data = await response.json();
  if (!data.ok) throw new Error(data.error ?? "request_failed");
  return data.result as T;
}
