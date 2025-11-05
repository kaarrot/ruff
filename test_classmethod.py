class A:
    test = 1

    def __init__(self):
        x = 1
    
    @classmethod
    def m1(cls):
        cls.test = 2

    def m2(self):
        self.x = 2

def bbb():
    aaa = 1
    aaa = aaa + 1


