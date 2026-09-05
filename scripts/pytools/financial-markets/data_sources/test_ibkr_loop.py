"""Test TDD per ibkr_loop.py -- SOLO la meccanica thread/asyncio (Step 2 del
brief Task 14). Nessuno di questi test tocca ib_async o la rete: sono
coroutine giocattolo che verificano l'isolamento fisico fra il thread
chiamante (quello di FastMCP, simulato qui dal thread principale di
pytest) e il thread dedicato che possiede il loop asyncio -- lo stesso
principio della sessione shell persistente di mcp-server, ma per Python.

Perche' questo file esiste separato da test_ibkr_source.py: ibkr_loop.py
non ha nulla di IBKR-specifico (vedi docstring di modulo in ibkr_loop.py),
quindi i suoi test non devono dipendere da ib_async/IbkrDataSource."""

import asyncio
import threading

import pytest

from data_sources.ibkr_loop import run_on_ibkr_loop


def test_run_on_ibkr_loop_returns_coroutine_result():
    async def coro():
        return 42

    assert run_on_ibkr_loop(coro) == 42


def test_run_on_ibkr_loop_runs_coroutine_on_a_different_thread_than_caller():
    # Prova diretta dell'isolamento: se questo test fallisse, la coroutine
    # girerebbe sul thread chiamante -- esattamente il bug che causa "This
    # event loop is already running" quando FastMCP e ib_async condividono
    # lo stesso loop/thread.
    caller_thread_id = threading.current_thread().ident
    captured = {}

    async def coro():
        captured["thread_id"] = threading.current_thread().ident

    run_on_ibkr_loop(coro)

    assert captured["thread_id"] is not None
    assert captured["thread_id"] != caller_thread_id


def test_run_on_ibkr_loop_handles_concurrent_calls_without_deadlock():
    # Due submission concorrenti da due thread chiamanti separati -- prova
    # che _start_lock (Step 1) protegge SOLO l'avvio del loop, non le
    # esecuzioni: se _start_lock restasse acquisito per l'intera durata di
    # run_on_ibkr_loop, la seconda chiamata si bloccherebbe per sempre in
    # attesa del lock (deadlock), mai raggiungendo run_coroutine_threadsafe.
    results = [None, None]
    errors = [None, None]

    def call(index, value):
        async def coro():
            await asyncio.sleep(0.05)
            return value

        try:
            results[index] = run_on_ibkr_loop(coro)
        except Exception as e:  # pragma: no cover - solo per diagnosi test
            errors[index] = e

    t1 = threading.Thread(target=call, args=(0, "a"))
    t2 = threading.Thread(target=call, args=(1, "b"))
    t1.start()
    t2.start()
    t1.join(timeout=5)
    t2.join(timeout=5)

    assert not t1.is_alive() and not t2.is_alive(), (
        "deadlock: un thread chiamante non e' terminato entro il timeout del test"
    )
    assert errors == [None, None]
    assert results == ["a", "b"]


def test_run_on_ibkr_loop_propagates_exception_from_coroutine():
    async def coro():
        raise ValueError("boom")

    with pytest.raises(ValueError, match="boom"):
        run_on_ibkr_loop(coro)


def test_run_on_ibkr_loop_raises_timeout_error_with_readable_message_on_slow_coroutine():
    async def slow_coro():
        await asyncio.sleep(1.0)
        return "mai raggiunto"

    with pytest.raises(TimeoutError, match="IBKR"):
        run_on_ibkr_loop(slow_coro, timeout=0.01)


def test_ibkr_loop_thread_is_marked_daemon():
    # daemon=True e' cio' che permette al processo Python di uscire pulito
    # quando il client MCP disconnette -- nessuno "spegne" mai esplicitamente
    # questo loop (vedi Step 1 del brief).
    import data_sources.ibkr_loop as ibkr_loop

    async def coro():
        return None

    run_on_ibkr_loop(coro)  # garantisce che il thread sia partito

    assert ibkr_loop._thread is not None
    assert ibkr_loop._thread.daemon is True
