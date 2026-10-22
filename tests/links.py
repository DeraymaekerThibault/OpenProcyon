"""Symlinks for tests: Windows only lets Developer Mode or an administrator create them, so tests that need one skip there."""


def symlink_or_skip(test, link, target):
    try:
        link.symlink_to(target)
    except OSError as e:
        test.skipTest(
            f"cannot create symlinks here ({e.strerror}); on Windows enable Developer Mode"
        )
