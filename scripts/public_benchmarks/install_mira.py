"""Install the pinned, checksum-verified Mira Linux x86_64 host for CI."""
import io
import platform
import tarfile
import urllib.request

from common import CACHE, PINS, atomic_write, sha256


def main():
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        raise SystemExit("install Mira 0.5.0 for your platform and pass --mira to run.py")
    url = f"https://github.com/everruns/mira/releases/download/{PINS['mira']}/mira-x86_64-unknown-linux-gnu.tar.gz"
    with urllib.request.urlopen(url, timeout=60) as response:
        data = response.read(10 * 1024 * 1024 + 1)
    if sha256(data) != PINS["mira_linux_archive_sha256"]:
        raise ValueError("Mira release checksum mismatch")
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        # Read exactly one member; never extract archive-controlled paths.
        data = archive.extractfile("mira").read(10 * 1024 * 1024 + 1)
    if len(data) > 10 * 1024 * 1024:
        raise ValueError("Mira executable exceeds limit")
    path = CACHE / "bin/mira"
    atomic_write(path, data)
    path.chmod(0o755)
    print(path)


if __name__ == "__main__":
    main()
