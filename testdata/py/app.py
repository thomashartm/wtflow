from fastapi import FastAPI
app = FastAPI()

class Repository:
    def transaction(self): ...
    def save(self, value): ...

class OCR:
    def recognize(self, document): ...

class Documents:
    def __init__(self, repo: Repository, ocr: OCR):
        self.repo = repo
        self.ocr = ocr

    async def process(self, document: str):
        if document == "":
            return None
        elif document == "skip":
            return False
        else:
            match document:
                case "invoice":
                    self.repo.save(document)
                case _:
                    self.repo.save("unknown")
        async with self.repo.transaction():
            self.ocr.recognize(document)
        return True

@app.post("/documents")
async def upload(document: str):
    return document
