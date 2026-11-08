/** Parse the versioned API envelope without exposing proxy HTML or raw bodies. */
export async function readApiResponse(response) {
  let value;
  try {
    const text = await response.text();
    if (!text.trim()) throw Error('empty');
    value = JSON.parse(text);
  } catch {
    throw Error(`The extension server returned an empty or incomplete response (HTTP ${response.status}). Retry the request. If it continues, check the server logs and reverse proxy limits.`);
  }
  if (!value || typeof value !== 'object' || typeof value.success !== 'boolean') {
    throw Error(`The extension server returned an invalid API response (HTTP ${response.status}). Check the server URL and update the server if needed.`);
  }
  return value;
}
