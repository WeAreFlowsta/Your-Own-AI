/**
 * Who hosts an online model and where: one line, the same on the Online
 * Models cards and in the model pickers. The catalog names the provider
 * that serves each model (`provider`), where that company is based, and the
 * maker when someone else made it (`maker`: a copy of Kimi served from the
 * United States is Moonshot's model, served by Fireworks).
 */
export type HostedModel = { provider?: string | null; maker?: string | null };
export type Host = { id: string; name: string; based_in?: string | null };

const named = (id: string, hosts: Host[]) => hosts.find((h) => h.id === id)?.name ?? id.charAt(0).toUpperCase() + id.slice(1);

export function hostLine(model: HostedModel, hosts: Host[]): string | null {
  if (!model.provider) return null;
  const host = hosts.find((h) => h.id === model.provider);
  const where = host?.based_in ? `, based in ${host.based_in}` : "";
  const served = `${named(model.provider, hosts)}${where}`;
  return model.maker && model.maker !== model.provider ? `Made by ${named(model.maker, hosts)}. Served by ${served}.` : `From ${served}.`;
}
