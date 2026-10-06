def control(items: list[str]) -> None:
    for item in items:
        if item == "skip":
            continue
        send(item)
    return


def send(item: str) -> None:
    print(item)
