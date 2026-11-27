import type { KromaClient } from '@kromatv/module-sdk';
import type { CloudLibrary } from './schemas';

type PickerHost = Partial<Pick<KromaClient['admin'], 'pickModuleFolder'>>;
type LocalFolder = Pick<CloudLibrary, 'local' | 'localDisplay'>;

/** Only a selection produces a mapping patch. A cancellation leaves even a dirty row alone. */
export async function pickLocalFolder(
  admin: PickerHost,
): Promise<LocalFolder | 'unavailable' | null> {
  // The client comes from the running web shell, not from the module's build.
  if (typeof admin?.pickModuleFolder !== 'function') {
    return 'unavailable';
  }
  const picked = await admin.pickModuleFolder('tv.kroma.cloud115');
  if (picked?.selected === false) {
    return null;
  }
  if (
    picked?.selected === true &&
    typeof picked.guestPath === 'string' && picked.guestPath.trim() &&
    typeof picked.displayPath === 'string' && picked.displayPath.trim()
  ) {
    return { local: picked.guestPath, localDisplay: picked.displayPath };
  }
  return 'unavailable';
}
