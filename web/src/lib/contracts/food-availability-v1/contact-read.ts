import { publicContact, type PublicContact } from './contact.ts';
// The writer's dedicated public disclosure is a final block, not prose scanning.
export function readPublicContact(content: unknown): PublicContact | undefined {
  if (typeof content !== 'string') return undefined;
  const marker = '\n\nPublic contact: ';
  const at = content.lastIndexOf(marker);
  if (at < 0) return undefined;
  const href = content.slice(at + marker.length);
  const type = href.startsWith('mailto:')
    ? 'email'
    : href.startsWith('tel:')
      ? 'phone'
      : 'https';
  const value =
    type === 'email' ? href.slice(7) : type === 'phone' ? href.slice(4) : href;
  const contact = publicContact({ type, value, public: true });
  return contact?.href === href ? contact : undefined;
}
