// Minimal JSON Schema (2020-12 subset) validator, enough for tokens.schema.json. No dependencies.
// Supports: $ref (#/...), type, const, enum, required, properties, additionalProperties (bool|schema), propertyNames,
// items, minItems, maxItems, minProperties, minLength, pattern, minimum, maximum, exclusiveMinimum.
export function validate(schema, value, root = schema, path = '$') {
  const errors = [];
  const err = (msg) => errors.push(`${path}: ${msg}`);

  if (schema.$ref) {
    const target = schema.$ref.replace(/^#\//, '').split('/').reduce((o, k) => o?.[k], root);
    if (!target) return [`${path}: unresolvable $ref ${schema.$ref}`];
    return validate(target, value, root, path);
  }

  const typeOf = (v) => (v === null ? 'null' : Array.isArray(v) ? 'array' : Number.isInteger(v) ? 'integer' : typeof v);
  if (schema.type) {
    const t = typeOf(value);
    const ok = schema.type === t || (schema.type === 'number' && t === 'integer');
    if (!ok) return [`${path}: expected ${schema.type}, got ${t}`];
  }
  if ('const' in schema && value !== schema.const) err(`expected const ${JSON.stringify(schema.const)}`);
  if (schema.enum && !schema.enum.includes(value)) err(`value ${JSON.stringify(value)} not in enum`);

  if (typeof value === 'string') {
    if (schema.minLength !== undefined && value.length < schema.minLength) err(`shorter than ${schema.minLength}`);
    if (schema.pattern && !new RegExp(schema.pattern).test(value)) err(`does not match ${schema.pattern}`);
  }
  if (typeof value === 'number') {
    if (schema.minimum !== undefined && value < schema.minimum) err(`< minimum ${schema.minimum}`);
    if (schema.maximum !== undefined && value > schema.maximum) err(`> maximum ${schema.maximum}`);
    if (schema.exclusiveMinimum !== undefined && value <= schema.exclusiveMinimum) err(`<= exclusiveMinimum ${schema.exclusiveMinimum}`);
  }
  if (Array.isArray(value)) {
    if (schema.minItems !== undefined && value.length < schema.minItems) err(`fewer than ${schema.minItems} items`);
    if (schema.maxItems !== undefined && value.length > schema.maxItems) err(`more than ${schema.maxItems} items`);
    if (schema.items) value.forEach((v, i) => errors.push(...validate(schema.items, v, root, `${path}[${i}]`)));
  }
  if (value && typeof value === 'object' && !Array.isArray(value)) {
    const keys = Object.keys(value);
    if (schema.minProperties !== undefined && keys.length < schema.minProperties) err(`fewer than ${schema.minProperties} properties`);
    for (const r of schema.required ?? []) if (!(r in value)) err(`missing required property "${r}"`);
    for (const k of keys) {
      if (schema.propertyNames) errors.push(...validate(schema.propertyNames, k, root, `${path}.<key ${k}>`));
      if (schema.properties && k in schema.properties) {
        errors.push(...validate(schema.properties[k], value[k], root, `${path}.${k}`));
      } else if (schema.additionalProperties === false) {
        err(`unexpected property "${k}"`);
      } else if (schema.additionalProperties && typeof schema.additionalProperties === 'object') {
        errors.push(...validate(schema.additionalProperties, value[k], root, `${path}.${k}`));
      }
    }
  }
  return errors;
}
