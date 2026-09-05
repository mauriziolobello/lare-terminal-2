import os

import local_dir


def test_resolve_uses_lare_local_dir_env_var_when_set(monkeypatch, tmp_path):
    monkeypatch.setenv("LARE_LOCAL_DIR", str(tmp_path))
    assert local_dir.resolve() == tmp_path


def test_resolve_falls_back_to_localappdata_when_lare_local_dir_unset(monkeypatch, tmp_path):
    monkeypatch.delenv("LARE_LOCAL_DIR", raising=False)
    monkeypatch.setenv("LOCALAPPDATA", str(tmp_path))
    assert local_dir.resolve() == tmp_path / "dev.lare.terminal"


def test_resolve_treats_empty_lare_local_dir_as_unset(monkeypatch, tmp_path):
    monkeypatch.setenv("LARE_LOCAL_DIR", "  ")
    monkeypatch.setenv("LOCALAPPDATA", str(tmp_path))
    assert local_dir.resolve() == tmp_path / "dev.lare.terminal"
