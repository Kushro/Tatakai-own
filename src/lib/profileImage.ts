/**
 * Profile images are used as-is. The upstream image-cache proxy
 * (api.tatakai.me/api/v3/images/profile) is not part of this fork.
 */
export function cachedProfileImageUrl(value?:string|null):string|undefined {
  return value||undefined;
}
