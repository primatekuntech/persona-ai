export class ApiError extends Error {
  constructor(
    public readonly status: number,
    public readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

async function errorFromResponse(res: Response): Promise<ApiError> {
  let code = "unknown";
  let message = `HTTP ${res.status}`;
  try {
    const body = await res.json();
    code = body?.error?.code ?? code;
    message = body?.error?.message ?? message;
  } catch {
    // non-JSON body — use defaults
  }
  return new ApiError(res.status, code, message);
}

export async function api<T = void>(
  path: string,
  init: RequestInit = {},
): Promise<T> {
  const method = (init.method ?? "GET").toUpperCase();
  const safeMethods = ["GET", "HEAD", "OPTIONS"];
  const csrfHeader: Record<string, string> = {};
  if (!safeMethods.includes(method)) {
    const match = document.cookie.match(/(?:^|;\s*)pai_csrf=([^;]+)/);
    if (match) csrfHeader["X-CSRF-Token"] = match[1];
  }

  const res = await fetch(path, {
    credentials: "include",
    headers: {
      "Content-Type": "application/json",
      ...csrfHeader,
      ...(init.headers as Record<string, string>),
    },
    ...init,
  });

  if (!res.ok) throw await errorFromResponse(res);
  if (res.status === 204) return undefined as T;
  return res.json() as Promise<T>;
}
