"""Fixture exercising the deps command (python)."""

import os
import numpy as np
from typing import Optional
from . import vendor_helpers
from .vendor_helpers import util
from .models import User
import myproject.models


def main() -> None:
    print(os.name, np.__version__, Optional, vendor_helpers, util, User, myproject)
