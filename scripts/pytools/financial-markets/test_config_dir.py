from pathlib import Path
import config_dir


def test_flag_with_value():
    assert config_dir.resolve(["server.py", "--config-dir", "D:/cfg"]) == Path("D:/cfg")


def test_flag_with_equals():
    assert config_dir.resolve(["server.py", "--config-dir=D:/cfg"]) == Path("D:/cfg")


def test_default_is_configuration_under_deploy_root():
    # server.py vive in <deploy_root>/pytools/<dominio>/ → default <deploy_root>/Configuration
    expected = Path(config_dir.__file__).resolve().parents[2] / "Configuration"
    assert config_dir.resolve(["server.py"]) == expected


def test_no_environment_variable_is_read(monkeypatch):
    monkeypatch.setenv("LARE_LOCAL_DIR", "D:/should-be-ignored")
    monkeypatch.setenv("LOCALAPPDATA", "D:/should-be-ignored-too")
    assert "should-be-ignored" not in str(config_dir.resolve(["server.py"]))
